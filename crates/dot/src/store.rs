use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha1::{Digest, Sha1};

use crate::git;
use crate::task::Task;

/// The git branch holding the tod storage, and nothing else.
pub const TOD_BRANCH: &str = "tod";

/// Name of the hidden worktree where the tod branch is checked out,
/// inside the repository's `.git` directory.
const WORKTREE_DIR: &str = "tod-worktree";

/// The tod database of one git repository: a dedicated `tod` branch,
/// checked out in a private worktree so it never pollutes the
/// working tree of the user.
pub struct Store {
    /// Root of the user's repository.
    root: PathBuf,
    /// The persistent worktree where the tod branch is checked out.
    tod_dir: PathBuf,
    /// Whether the tod branch was already pulled from origin in this
    /// Store instance: one command = one Store = one pull.
    pulled: Cell<bool>,
    /// Whether commits happened that still need a push, flushed once
    /// when the Store (the command) is dropped.
    dirty: Cell<bool>,
    /// Whether the final push runs in the background (the command
    /// returns instantly) or blocks. Background by default from
    /// `Store::discover` (the CLI), blocking from `Store::new`.
    async_push: Cell<bool>,
    /// Cached "does an origin remote exist", looked up once per Store.
    has_origin: Cell<Option<bool>>,
}

impl Store {
    /// Discover the store of the git repository containing the
    /// current directory. CLI entry point: the final push runs in
    /// the background so commands return instantly
    /// (`TOD_SYNC_PUSH=sync` restores a blocking push).
    pub fn discover() -> Result<Self> {
        let store = Self::new(git::repo_root()?);
        if std::env::var("TOD_SYNC_PUSH").as_deref() != Ok("sync") {
            store.async_push.set(true);
        }
        Ok(store)
    }

    /// Open the store of an explicit repository root. The worktree
    /// path is derived from the git layout; the branch itself is only
    /// materialized by `init` or, later, on first use.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // Best effort: a non-git directory gets a dead worktree path,
        // and every operation fails cleanly with "not initialized".
        let tod_dir = git::common_dir(&root)
            .map(|d| d.join(WORKTREE_DIR))
            .unwrap_or_else(|_| root.join(".git").join(WORKTREE_DIR));
        Store {
            root,
            tod_dir,
            pulled: Cell::new(false),
            dirty: Cell::new(false),
            async_push: Cell::new(false),
            has_origin: Cell::new(None),
        }
    }

    /// Choose whether the final push runs in the background (the
    /// command returns instantly) or blocks until done. The CLI
    /// enables the background mode through `Store::discover`.
    pub fn set_async_push(&self, yes: bool) {
        self.async_push.set(yes);
    }

    /// The user's repository root: where the agent skills live.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The persistent worktree of the tod branch.
    pub fn tod_dir(&self) -> &Path {
        &self.tod_dir
    }

    pub fn todo_dir(&self) -> PathBuf {
        self.tod_dir.join("todo")
    }

    pub fn backlog_dir(&self) -> PathBuf {
        self.tod_dir.join("backlog")
    }

    /// The central plan document of the project.
    pub fn plan_path(&self) -> PathBuf {
        self.tod_dir.join(crate::plan::PLAN_FILE)
    }

    /// The folder holding the generated spec markdowns.
    pub fn md_dir(&self) -> PathBuf {
        self.tod_dir.join("md")
    }

    /// Directory holding the tasks of one branch (context).
    pub fn context_dir(&self, branch: &str) -> PathBuf {
        self.todo_dir().join(sanitize(branch))
    }

    /// Current branch of the repository behind this store: the user's
    /// working context, not the tod branch.
    pub fn branch(&self) -> Result<String> {
        git::current_branch_in(&self.root)
    }

    /// Has 'tod init' been run in this repository (or is the tod
    /// branch available, locally or on the origin remote)?
    pub fn is_initialized(&self) -> bool {
        self.branch_available().unwrap_or(false)
    }

    /// The tod branch exists locally, or as a remote-tracking ref
    /// after a fresh clone.
    fn branch_available(&self) -> Result<bool> {
        if git::branch_exists(&self.root, TOD_BRANCH)? {
            return Ok(true);
        }
        git::ref_exists(&self.root, &format!("refs/remotes/origin/{TOD_BRANCH}"))
    }

    /// Adopt the tod branch after a clone: create the local branch
    /// from its origin counterpart.
    fn ensure_branch(&self) -> Result<()> {
        if git::branch_exists(&self.root, TOD_BRANCH)? {
            return Ok(());
        }
        if git::ref_exists(&self.root, &format!("refs/remotes/origin/{TOD_BRANCH}"))? {
            git::create_branch_from(&self.root, TOD_BRANCH, &format!("origin/{TOD_BRANCH}"))?;
            return Ok(());
        }
        bail!("tod is not initialized here. Run 'tod init' first.");
    }

    /// Is the persistent worktree of the tod branch in place?
    /// A linked worktree contains a `.git` *file*, not a directory.
    fn worktree_ready(&self) -> bool {
        self.tod_dir.join(".git").is_file()
    }

    /// Make sure the tod branch is checked out in the persistent
    /// worktree, and up to date with origin. Safe to call before
    /// every operation; the pull only runs once per Store instance
    /// (one command = one Store).
    pub fn ensure_init(&self) -> Result<()> {
        self.ensure_branch()?;
        if !self.worktree_ready() {
            git::worktree_prune(&self.root)?;
            git::worktree_add(&self.root, &self.tod_dir, TOD_BRANCH)?;
        }
        self.sync_pull()?;
        Ok(())
    }

    /// Create the tod branch and its persistent worktree. Idempotent.
    pub fn init(&self) -> Result<()> {
        if !git::branch_exists(&self.root, TOD_BRANCH)? {
            if git::ref_exists(&self.root, &format!("refs/remotes/origin/{TOD_BRANCH}"))? {
                git::create_branch_from(&self.root, TOD_BRANCH, &format!("origin/{TOD_BRANCH}"))?;
            } else {
                git::create_orphan_branch(&self.root, TOD_BRANCH)?;
            }
        }
        if !self.worktree_ready() {
            git::worktree_prune(&self.root)?;
            git::worktree_add(&self.root, &self.tod_dir, TOD_BRANCH)?;
        }
        Ok(())
    }

    /// Commit the pending changes of the tod worktree. No-op when
    /// nothing changed. The push to origin is deferred to the end of
    /// the command (see `Drop for Store`), so a command making
    /// several commits still performs a single network round trip.
    pub(crate) fn commit(&self, message: &str) -> Result<()> {
        if git::commit_all(&self.tod_dir, message)? {
            self.dirty.set(true);
        }
        Ok(())
    }

    /// Does the repository have an 'origin' remote? Looked up once
    /// per Store instance.
    fn has_origin(&self) -> bool {
        if let Some(known) = self.has_origin.get() {
            return known;
        }
        let known = git::remotes(&self.tod_dir)
            .map(|r| r.iter().any(|r| r == "origin"))
            .unwrap_or(false);
        self.has_origin.set(Some(known));
        known
    }

    /// File holding the time of the last successful pull, used to
    /// skip fetches when the branch is fresh enough. Lives next to
    /// the worktree, inside the `.git` directory.
    fn pull_marker(&self) -> Option<PathBuf> {
        self.tod_dir.parent().map(|p| p.join("tod-last-pull"))
    }

    /// Was the tod branch pulled from origin within the last 'ttl'
    /// seconds?
    fn pulled_recently(&self, ttl: u64) -> bool {
        let Some(marker) = self.pull_marker() else {
            return false;
        };
        let Ok(modified) = std::fs::metadata(&marker).and_then(|m| m.modified()) else {
            return false;
        };
        modified
            .elapsed()
            .map(|e| e.as_secs() < ttl)
            .unwrap_or(false)
    }

    /// File holding the time of the last background push attempt. A
    /// fresh marker means the unpushed commits are still on their
    /// way, not stuck. Lives next to the worktree, inside the `.git`
    /// directory.
    fn push_marker(&self) -> Option<PathBuf> {
        self.tod_dir.parent().map(|p| p.join("tod-last-push"))
    }

    /// Was a background push started within the last 'ttl' seconds?
    fn pushed_recently(&self, ttl: u64) -> bool {
        let Some(marker) = self.push_marker() else {
            return false;
        };
        let Ok(modified) = std::fs::metadata(&marker).and_then(|m| m.modified()) else {
            return false;
        };
        modified
            .elapsed()
            .map(|e| e.as_secs() < ttl)
            .unwrap_or(false)
    }

    /// Remember that a background push is on its way.
    fn mark_push(&self) {
        if let Some(marker) = self.push_marker() {
            let _ = std::fs::File::create(&marker);
        }
    }

    /// Fetch origin and fast-forward the tod branch before a command
    /// reads or writes it, so every machine works on the latest
    /// todos. The fetch is skipped when the last one is fresher than
    /// the TOD_SYNC_TTL (default 30s; 0 = always pull), so repeated
    /// commands stay instant. An unreachable remote is a warning
    /// (offline work); a divergence between the local and the remote
    /// branch is a hard error.
    fn sync_pull(&self) -> Result<()> {
        if self.pulled.get() {
            return Ok(());
        }
        self.pulled.set(true);
        if !self.has_origin() || std::env::var_os("TOD_NO_SYNC").is_some() {
            return Ok(());
        }
        let ttl = std::env::var("TOD_SYNC_TTL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        if ttl > 0 && self.pulled_recently(ttl) {
            return Ok(());
        }
        if git::fetch(&self.tod_dir, "origin").is_err() {
            eprintln!("warning: cannot reach origin, working on the local '{TOD_BRANCH}' branch");
            return Ok(());
        }
        let origin_ref = format!("origin/{TOD_BRANCH}");
        if git::ref_exists(&self.tod_dir, &format!("refs/remotes/{origin_ref}"))? {
            git::merge_ff_only(&self.tod_dir, &origin_ref).with_context(|| {
                format!(
                    "the '{TOD_BRANCH}' branch diverged from {origin_ref}; resolve it manually in {}",
                    self.tod_dir.display()
                )
            })?;
            // commits that a previous background push failed to
            // flush: when a burst of commands races, the pushes are
            // still on their way — only complain when none was
            // started recently
            if git::ahead_of(&self.tod_dir, &origin_ref, TOD_BRANCH)? > 0
                && !self.pushed_recently(60)
            {
                eprintln!(
                    "warning: the '{TOD_BRANCH}' branch has unpushed commits, retrying the push in the background"
                );
                let _ = git::spawn_push(&self.tod_dir, "origin", TOD_BRANCH);
                self.mark_push();
            }
        }
        // mark the pull as fresh
        if let Some(marker) = self.pull_marker() {
            let _ = std::fs::File::create(&marker);
        }
        Ok(())
    }

    /// Create a new task in the current context, or in the backlog.
    pub fn add(&self, description: &str, author: &str, backlog: bool) -> Result<Task> {
        self.ensure_init()?;
        let context = if backlog { None } else { Some(self.branch()?) };
        self.add_in(context.as_deref(), description, author)
    }

    /// Create a new task in an explicit context, or in the backlog
    /// when the context is None. Used by the server, which manages
    /// every context at once.
    pub fn add_in(&self, context: Option<&str>, description: &str, author: &str) -> Result<Task> {
        self.ensure_init()?;
        let now = chrono::Utc::now();
        let task = Task {
            uid: new_uid(description),
            description: description.to_string(),
            author: author.to_string(),
            created: now,
            updated: now,
            done: false,
            content: None,
        };
        let dir = match context {
            Some(branch) => self.context_dir(branch),
            None => self.backlog_dir(),
        };
        fs::create_dir_all(&dir)?;
        write_task(&dir, &task)?;
        self.commit(&format!("tod: add {} {}", &task.uid[..7], description))?;
        Ok(task)
    }

    /// Load the backlog.
    pub fn load_backlog(&self) -> Result<Vec<Task>> {
        self.ensure_init()?;
        load_dir(&self.backlog_dir())
    }

    /// Load the tasks of one context.
    pub fn load_context(&self, branch: &str) -> Result<Vec<Task>> {
        self.ensure_init()?;
        load_dir(&self.context_dir(branch))
    }

    /// Every context (branch) holding at least one task, sorted by name.
    pub fn contexts(&self) -> Result<Vec<(String, Vec<Task>)>> {
        self.ensure_init()?;
        let mut out = Vec::new();
        if !self.todo_dir().is_dir() {
            return Ok(out);
        }
        for entry in fs::read_dir(self.todo_dir())?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let tasks = load_dir(&path)?;
                if !tasks.is_empty() {
                    let name = desanitize(&entry.file_name().to_string_lossy());
                    out.push((name, tasks));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Find a task by (possibly partial) identifier, searching the
    /// backlog first, then every context.
    /// Returns (location, task) where location is a branch or "backlog".
    pub fn find_task(&self, identifier: &str) -> Result<(String, Task)> {
        self.ensure_init()?;
        let backlog = self.load_backlog()?;
        if let Ok(task) = match_task(&backlog, identifier) {
            return Ok(("backlog".to_string(), task.clone()));
        }
        for (context, tasks) in self.contexts()? {
            if let Ok(task) = match_task(&tasks, identifier) {
                return Ok((context, task.clone()));
            }
        }
        bail!("no task found with identifier '{identifier}'")
    }

    /// Mark a task of the current context as complete.
    pub fn complete(&self, identifier: &str) -> Result<Task> {
        self.ensure_init()?;
        let dir = self.context_dir(&self.branch()?);
        let tasks = load_dir(&dir)?;
        let mut task = match_task(&tasks, identifier)?.clone();
        if task.done {
            bail!("task #{} is already complete", &task.uid[..5]);
        }
        task.done = true;
        task.updated = chrono::Utc::now();
        write_task(&dir, &task)?;
        self.commit(&format!("tod: complete {} {}", &task.uid[..7], task.description))?;
        Ok(task)
    }

    /// Delete a task from the current context, or from the backlog.
    pub fn remove(&self, identifier: &str, backlog: bool) -> Result<Task> {
        self.ensure_init()?;
        let dir = if backlog {
            self.backlog_dir()
        } else {
            self.context_dir(&self.branch()?)
        };
        let tasks = load_dir(&dir)?;
        let task = match_task(&tasks, identifier)?.clone();
        fs::remove_file(dir.join(&task.uid))?;
        self.commit(&format!("tod: remove {} {}", &task.uid[..7], task.description))?;
        Ok(task)
    }

    /// Move a task between the backlog and the current context.
    pub fn move_task(&self, identifier: &str, to_backlog: bool) -> Result<Task> {
        self.ensure_init()?;
        let branch = self.branch()?;
        let (from, to) = if to_backlog {
            (self.context_dir(&branch), self.backlog_dir())
        } else {
            (self.backlog_dir(), self.context_dir(&branch))
        };
        let tasks = load_dir(&from)?;
        let task = match_task(&tasks, identifier)?.clone();
        fs::create_dir_all(&to)?;
        fs::rename(from.join(&task.uid), to.join(&task.uid))?;
        let destination = if to_backlog { "backlog" } else { "context" };
        self.commit(&format!("tod: move {} to {destination}", &task.uid[..7]))?;
        Ok(task)
    }

    /// Move a task from the backlog or any context into an explicit
    /// context. Used by 'tod start', which gives a task its own
    /// working branch.
    pub fn move_task_to(&self, identifier: &str, to_context: &str) -> Result<Task> {
        self.ensure_init()?;
        let (from_context, task) = self.find_task(identifier)?;
        let from = match from_context.as_str() {
            "backlog" => self.backlog_dir(),
            context => self.context_dir(context),
        };
        if from == self.context_dir(to_context) {
            return Ok(task); // already in the target context
        }
        let to = self.context_dir(to_context);
        fs::create_dir_all(&to)?;
        fs::rename(from.join(&task.uid), to.join(&task.uid))
            .with_context(|| format!("cannot move task #{}", &task.uid[..7]))?;
        self.commit(&format!("tod: start {} in {to_context}", &task.uid[..7]))?;
        Ok(task)
    }

    /// Close a context: delete every task of the branch and its folder.
    pub fn resolve(&self, context: &str) -> Result<usize> {
        self.ensure_init()?;
        let dir = self.context_dir(context);
        if !dir.is_dir() {
            bail!("unknown context '{context}'");
        }
        let count = load_dir(&dir)?.len();
        fs::remove_dir_all(&dir).with_context(|| format!("cannot remove {}", dir.display()))?;
        self.commit(&format!("tod: resolve context {context}"))?;
        Ok(count)
    }

    /// Export the whole database (backlog + every context) as a JSON file.
    /// Returns (backlog size, context count).
    pub fn export(&self, output: &str) -> Result<(usize, usize)> {
        self.ensure_init()?;
        let backlog = self.load_backlog()?;
        let contexts = self.contexts()?;
        let mut map = serde_json::Map::new();
        for (name, tasks) in &contexts {
            map.insert(name.clone(), serde_json::to_value(tasks)?);
        }
        let db = serde_json::json!({ "backlog": backlog, "contexts": map });
        fs::write(output, serde_json::to_string_pretty(&db)?)
            .with_context(|| format!("cannot write {output}"))?;
        Ok((backlog.len(), contexts.len()))
    }
}

impl Drop for Store {
    /// Flush the tod branch once per command, at the very end, and
    /// only when something was actually committed. By default from
    /// the CLI the push runs in the background: the command returns
    /// instantly, a failed push is detected and retried by the next
    /// command. A blocking push failure (offline, no permission) is
    /// a warning: the commits are already local.
    /// TOD_NO_SYNC=1 disables the push entirely.
    fn drop(&mut self) {
        if !self.dirty.get() || std::env::var_os("TOD_NO_SYNC").is_some() {
            return;
        }
        if !self.has_origin() {
            return;
        }
        let push = if self.async_push.get() {
            self.mark_push();
            git::spawn_push(&self.tod_dir, "origin", TOD_BRANCH)
        } else {
            git::push(&self.tod_dir, "origin", TOD_BRANCH)
        };
        if let Err(e) = push {
            eprintln!("warning: {e}");
        }
    }
}

/// Load every task file from a directory. The file name is the uid.
/// A missing directory is an empty task list; a corrupted file is
/// skipped with a warning instead of failing the whole command.
pub fn load_dir(dir: &Path) -> Result<Vec<Task>> {
    let mut tasks = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(tasks),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let uid = entry.file_name().to_string_lossy().to_string();
        match fs::read_to_string(&path) {
            Ok(content) => match Task::parse(&uid, &content) {
                Ok(task) => tasks.push(task),
                Err(_) => eprintln!("warning: skipping invalid task file {}", path.display()),
            },
            Err(_) => eprintln!("warning: cannot read {}", path.display()),
        }
    }
    tasks.sort_by(|a, b| a.created.cmp(&b.created));
    Ok(tasks)
}

/// Match a (possibly partial) identifier against task uids, git-style.
/// Returns the unique match, or an error when nothing or several match.
pub fn match_task<'a>(tasks: &'a [Task], identifier: &str) -> Result<&'a Task> {
    let matches: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.uid.starts_with(identifier))
        .collect();
    match matches.len() {
        0 => bail!("no task found with identifier '{identifier}'"),
        1 => Ok(matches[0]),
        n => bail!("identifier '{identifier}' is ambiguous ({n} matches)"),
    }
}

/// Write a task to its file inside 'dir'.
pub fn write_task(dir: &Path, task: &Task) -> Result<()> {
    let path = dir.join(&task.uid);
    fs::write(&path, task.stringify())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

/// Branch names may contain '/' which cannot be a single directory name.
pub fn sanitize(branch: &str) -> String {
    branch.replace('/', "--")
}

pub fn desanitize(dir: &str) -> String {
    dir.replace("--", "/")
}

fn new_uid(description: &str) -> String {
    let now = chrono::Utc::now().to_rfc3339();
    let mut hasher = Sha1::new();
    hasher.update(description.as_bytes());
    hasher.update(now.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A temp directory with a real git repository and a store on it.
    fn git_repo() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
        assert!(out.status.success(), "git init failed");
        let store = Store::new(dir.path());
        (dir, store)
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {} failed", args.join(" "));
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn sanitize_round_trip() {
        assert_eq!(sanitize("feature/fix"), "feature--fix");
        assert_eq!(sanitize("main"), "main");
        assert_eq!(desanitize(&sanitize("feature/fix")), "feature/fix");
    }

    #[test]
    fn uids_are_unique() {
        // Same description, consecutive calls: uids must still differ.
        assert_ne!(new_uid("same"), new_uid("same"));
    }

    #[test]
    fn match_task_accepts_prefix_and_full_id() {
        let mk = |uid: &str| Task {
            uid: uid.to_string(),
            description: "d".to_string(),
            author: "a".to_string(),
            created: chrono::Utc::now(),
            updated: chrono::Utc::now(),
            done: false,
            content: None,
        };
        let tasks = vec![mk("abcdef1234"), mk("abz4567890")];
        assert_eq!(match_task(&tasks, "abcdef1234").unwrap().uid, "abcdef1234");
        assert_eq!(match_task(&tasks, "abc").unwrap().uid, "abcdef1234");
        assert_eq!(match_task(&tasks, "abz").unwrap().uid, "abz4567890");
        assert!(match_task(&tasks, "zzz").is_err());
        let ambiguous = vec![mk("abcdef1234"), mk("abc4567890")];
        assert!(match_task(&ambiguous, "abc").is_err());
    }

    #[test]
    fn load_dir_on_missing_directory_is_empty() {
        assert!(load_dir(Path::new("/nonexistent-tod-dir")).unwrap().is_empty());
    }

    #[test]
    fn init_creates_branch_and_worktree_and_guards() {
        let (dir, store) = git_repo();
        assert!(!store.is_initialized());
        assert!(store.ensure_init().is_err());

        store.init().unwrap();
        assert!(store.is_initialized());
        store.ensure_init().unwrap();

        // the worktree lives in the .git directory, hidden from the user
        assert!(store.tod_dir().join(".git").is_file());
        let git_dir = git::common_dir(dir.path()).unwrap();
        assert!(store.tod_dir().starts_with(&git_dir));
        assert!(!dir.path().join(".tod").exists());

        // init is idempotent
        store.init().unwrap();

        // without a git repository, nothing works
        let bare = tempfile::tempdir().unwrap();
        let store = Store::new(bare.path());
        assert!(store.init().is_err());
        assert!(store.ensure_init().is_err());
    }

    #[test]
    fn worktree_is_recreated_after_manual_deletion() {
        let (_dir, store) = git_repo();
        store.init().unwrap();
        let tod_dir = store.tod_dir().to_path_buf();
        std::fs::remove_dir_all(&tod_dir).unwrap();
        assert!(!tod_dir.exists());

        store.add_in(Some("main"), "still works", "tester").unwrap();
        assert!(tod_dir.join(".git").is_file());
        assert_eq!(store.load_context("main").unwrap().len(), 1);
    }

    #[test]
    fn add_in_creates_tasks_in_context_and_backlog() {
        let (_dir, store) = git_repo();
        store.init().unwrap();

        let in_context = store.add_in(Some("feature/server"), "in context", "tester").unwrap();
        let in_backlog = store.add_in(None, "in backlog", "tester").unwrap();

        assert!(store.context_dir("feature/server").join(&in_context.uid).is_file());
        assert!(store.backlog_dir().join(&in_backlog.uid).is_file());
        assert_eq!(store.load_context("feature/server").unwrap().len(), 1);
        assert_eq!(store.load_backlog().unwrap().len(), 1);

        let (context, task) = store.find_task(&in_context.uid).unwrap();
        assert_eq!(context, "feature/server");
        assert_eq!(task.description, "in context");
    }

    #[test]
    fn mutations_are_committed_to_the_tod_branch() {
        let (dir, store) = git_repo();
        store.init().unwrap();
        let task = store.add_in(None, "committed task", "tester").unwrap();

        let log = git(dir.path(), &["log", "--format=%s", TOD_BRANCH]);
        assert!(log.contains("tod: init"));
        assert!(log.contains(&format!("tod: add {}", &task.uid[..7])));

        store.complete(&task.uid[..7]).unwrap_err(); // not in the current context
        store.load_backlog().unwrap();
        assert_eq!(store.load_backlog().unwrap().len(), 1);
    }

    #[test]
    fn tod_branch_contains_only_tod_files() {
        let (dir, store) = git_repo();
        // user content on the working branch
        std::fs::write(dir.path().join("code.txt"), "user code").unwrap();
        store.init().unwrap();
        store.add_in(Some("main"), "a task", "tester").unwrap();

        let files = git(dir.path(), &["ls-tree", "-r", "--name-only", TOD_BRANCH]);
        assert_eq!(files, format!("todo/main/{}", store.load_context("main").unwrap()[0].uid));
        assert!(!files.contains("code.txt"));

        // and the user's working tree is untouched
        assert!(!dir.path().join("todo").exists());
    }

    #[test]
    fn move_task_to_moves_between_backlog_and_contexts() {
        let (_dir, store) = git_repo();
        store.init().unwrap();
        let backlog_task = store.add_in(None, "from backlog", "tester").unwrap();
        let context_task = store.add_in(Some("main"), "from context", "tester").unwrap();

        // backlog -> context
        store.move_task_to(&backlog_task.uid[..7], "fix-login").unwrap();
        assert!(store.load_backlog().unwrap().is_empty());
        assert_eq!(store.load_context("fix-login").unwrap().len(), 1);

        // context -> other context
        store.move_task_to(&context_task.uid[..7], "fix-login").unwrap();
        assert!(store.load_context("main").unwrap().is_empty());
        assert_eq!(store.load_context("fix-login").unwrap().len(), 2);

        // moving to the context the task is already in is a no-op
        store.move_task_to(&context_task.uid[..7], "fix-login").unwrap();
        assert_eq!(store.load_context("fix-login").unwrap().len(), 2);
    }

    #[test]
    fn tod_branch_is_adopted_after_a_clone() {
        let (origin, store) = git_repo();
        store.init().unwrap();
        store.add_in(None, "shared task", "tester").unwrap();

        let clone_dir = tempfile::tempdir().unwrap();
        let out = Command::new("git")
            .args(["clone", origin.path().to_string_lossy().as_ref(), clone_dir.path().to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git clone failed");

        let clone = Store::new(clone_dir.path());
        assert!(clone.is_initialized()); // via origin/tod
        clone.ensure_init().unwrap();
        let tasks = clone.load_backlog().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description, "shared task");
    }

    #[test]
    fn mutations_sync_between_machines_via_the_remote() {
        // a bare repository plays the role of the shared server
        let bare = tempfile::tempdir().unwrap();
        let server = bare.path().join("server.git");
        let out = Command::new("git")
            .args(["init", "--bare", server.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");

        // machine A initializes tod and adds a task: the push is
        // flushed when the command (the Store) ends
        let (dir_a, _store_a) = git_repo();
        let out = Command::new("git")
            .args(["remote", "add", "origin", server.to_string_lossy().as_ref()])
            .current_dir(dir_a.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git remote add failed");
        {
            let store_a = Store::new(dir_a.path());
            store_a.init().unwrap();
            store_a.add_in(None, "from A", "A").unwrap();
        }
        assert!(git::ref_exists(&server, "refs/heads/tod").unwrap());

        // machine B is a fresh clone: it pulls A's task on its first command
        let dir_b = tempfile::tempdir().unwrap();
        let out = Command::new("git")
            .args(["clone", server.to_string_lossy().as_ref(), dir_b.path().to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git clone failed");
        {
            let store_b = Store::new(dir_b.path());
            assert_eq!(store_b.load_backlog().unwrap().len(), 1);
            store_b.add_in(None, "from B", "B").unwrap();
        }

        // machine A's next command pulls B's task once the freshness
        // marker is stale (simulate the TTL elapsing: remove it)
        let _ = std::fs::remove_file(
            git::common_dir(dir_a.path()).unwrap().join("tod-last-pull"),
        );
        let store_a2 = Store::new(dir_a.path());
        assert_eq!(store_a2.load_backlog().unwrap().len(), 2);
    }

    #[test]
    fn push_is_flushed_once_at_the_end_of_a_command() {
        // a bare repository plays the role of the shared server
        let bare = tempfile::tempdir().unwrap();
        let server = bare.path().join("server.git");
        let out = Command::new("git")
            .args(["init", "--bare", server.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");

        let (dir, _store) = git_repo();
        let out = Command::new("git")
            .args(["remote", "add", "origin", server.to_string_lossy().as_ref()])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git remote add failed");
        {
            let store = Store::new(dir.path());
            store.init().unwrap();
            store.add_in(None, "first", "tester").unwrap();
            store.add_in(None, "second", "tester").unwrap();
            // both commits are local: the remote is not touched yet
            assert!(!git::ref_exists(&server, "refs/heads/tod").unwrap());
        }
        // the Store dropped: one single push flushed every commit
        assert!(git::ref_exists(&server, "refs/heads/tod").unwrap());
        let commits = git(&server, &["rev-list", "--count", "tod"]);
        assert_eq!(commits, "3"); // tod: init + two additions
    }

    #[test]
    fn pull_freshness_marker_skips_repeated_fetches() {
        // a bare repository plays the role of the shared server
        let bare = tempfile::tempdir().unwrap();
        let server = bare.path().join("server.git");
        let out = Command::new("git")
            .args(["init", "--bare", server.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");

        let (dir, _store) = git_repo();
        let out = Command::new("git")
            .args(["remote", "add", "origin", server.to_string_lossy().as_ref()])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git remote add failed");
        {
            let store = Store::new(dir.path());
            store.init().unwrap();
            store.add_in(None, "first", "tester").unwrap();
        }

        // a pull leaves a freshness marker next to the worktree
        let store = Store::new(dir.path());
        store.ensure_init().unwrap();
        assert!(store.pulled_recently(30)); // fresh: within the TTL
        assert!(!store.pulled_recently(0)); // ttl=0 means "always pull"
        let git_dir = git::common_dir(dir.path()).unwrap();
        assert!(git_dir.join("tod-last-pull").is_file());
    }

    /// Wait until 'predicate' is true, polling for up to 10 seconds:
    /// a background push needs a moment to land on the remote.
    fn eventually(predicate: impl Fn() -> bool) -> bool {
        for _ in 0..200 {
            if predicate() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn background_push_flushes_without_blocking_the_command() {
        // a bare repository plays the role of the shared server
        let bare = tempfile::tempdir().unwrap();
        let server = bare.path().join("server.git");
        let out = Command::new("git")
            .args(["init", "--bare", server.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");

        let (dir, _store) = git_repo();
        let out = Command::new("git")
            .args(["remote", "add", "origin", server.to_string_lossy().as_ref()])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git remote add failed");
        {
            let store = Store::new(dir.path());
            store.set_async_push(true);
            store.init().unwrap();
            store.add_in(None, "pushed later", "tester").unwrap();
        }
        // the command is over: the background push lands on its own
        assert!(eventually(|| git::ref_exists(&server, "refs/heads/tod").unwrap()));
        let commits = git(&server, &["rev-list", "--count", "tod"]);
        assert_eq!(commits, "2"); // tod: init + the addition
    }

    #[test]
    fn unpushed_commits_are_detected_and_retried_by_the_next_command() {
        // a bare repository plays the role of the shared server
        let bare = tempfile::tempdir().unwrap();
        let server = bare.path().join("server.git");
        let out = Command::new("git")
            .args(["init", "--bare", server.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");

        let (dir, _store) = git_repo();
        let out = Command::new("git")
            .args(["remote", "add", "origin", server.to_string_lossy().as_ref()])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git remote add failed");
        {
            let store = Store::new(dir.path());
            store.init().unwrap();
            store.add_in(None, "pushed once", "tester").unwrap();
        }
        assert!(eventually(|| {
            git(&server, &["rev-list", "--count", "tod"]) == "2"
        }));

        // simulate a background push that failed: a commit lands on
        // the local tod branch without ever reaching the remote
        let worktree = git::common_dir(dir.path()).unwrap().join("tod-worktree");
        git(
            dir.path(),
            &[
                "-C",
                worktree.to_string_lossy().as_ref(),
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "--allow-empty",
                "-m",
                "stray commit",
            ],
        );
        assert_eq!(
            git::ahead_of(&worktree, "origin/tod", "tod").unwrap(),
            1
        );

        // the next command fetches (marker stale), sees the local
        // branch ahead of origin, and starts a push in the background
        let _ = std::fs::remove_file(
            git::common_dir(dir.path()).unwrap().join("tod-last-pull"),
        );
        let store = Store::new(dir.path());
        store.load_backlog().unwrap();
        assert!(eventually(|| {
            git(&server, &["rev-list", "--count", "tod"]) == "3"
        }));
    }
    #[test]
    fn store_works_on_a_bare_repository() {
        // a normal repository with tod data
        let (origin, store) = git_repo();
        store.init().unwrap();
        store.add_in(Some("main"), "task in bare", "tester").unwrap();

        // a bare mirror of it — how the server stores repositories
        let bare = tempfile::tempdir().unwrap();
        let mirror = bare.path().join("mirror.git");
        let out = Command::new("git")
            .args([
                "clone",
                "--mirror",
                origin.path().to_string_lossy().as_ref(),
                mirror.to_string_lossy().as_ref(),
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "git clone --mirror failed");

        let bare_store = Store::new(&mirror);
        bare_store.ensure_init().unwrap();
        assert!(bare_store.is_initialized());
        assert_eq!(bare_store.branch().unwrap(), "main"); // HEAD of the bare repo
        assert_eq!(bare_store.load_context("main").unwrap().len(), 1);

        // mutations commit in the worktree attached to the bare repo
        bare_store.add_in(None, "from the server", "web").unwrap();
        assert_eq!(bare_store.load_backlog().unwrap().len(), 1);
        let git_dir = git::common_dir(&mirror).unwrap();
        assert!(git_dir.join("tod-worktree").join(".git").is_file());
    }

    #[test]
    fn unreachable_remote_is_a_warning_not_an_error() {
        let (dir, store) = git_repo();
        let out = Command::new("git")
            .args(["remote", "add", "origin", "/nonexistent/tod-remote.git"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git remote add failed");

        store.init().unwrap();
        store.add_in(None, "offline task", "tester").unwrap();
        assert_eq!(store.load_backlog().unwrap().len(), 1);
    }
}
