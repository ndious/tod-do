use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Run git in 'dir' and return its trimmed stdout, or the error
/// message on failure.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

/// Root of the current git repository.
pub fn repo_root() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        bail!("not inside a git repository");
    }
    let path = String::from_utf8(out.stdout)?.trim().to_string();
    Ok(PathBuf::from(path))
}

/// The shared `.git` directory of the repository rooted at 'root'
/// (works from the main worktree and from linked worktrees alike).
pub fn common_dir(root: &Path) -> Result<PathBuf> {
    let dir = git(root, &["rev-parse", "--git-common-dir"])?;
    let path = PathBuf::from(dir);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(root.join(path))
    }
}

/// Does a (local or remote-tracking) ref exist, e.g. "refs/heads/tod".
pub fn ref_exists(root: &Path, reference: &str) -> Result<bool> {
    let out = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", reference])
        .current_dir(root)
        .output()
        .context("failed to run git")?;
    Ok(out.status.success())
}

/// Does the local branch exist?
pub fn branch_exists(root: &Path, branch: &str) -> Result<bool> {
    ref_exists(root, &format!("refs/heads/{branch}"))
}

/// Create a branch holding nothing but an empty root commit,
/// disconnected from the repository history. Used for the tod
/// branch, which must never contain the project's code.
pub fn create_orphan_branch(root: &Path, branch: &str) -> Result<()> {
    let empty_tree = Command::new("git")
        .args(["mktree"])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .context("failed to run git mktree")?;
    if !empty_tree.status.success() {
        bail!("git mktree failed");
    }
    let tree = String::from_utf8(empty_tree.stdout)?.trim().to_string();
    let commit = git(root, &["commit-tree", &tree, "-m", "tod: init"])?;
    git(root, &["update-ref", &format!("refs/heads/{branch}"), &commit])
        .with_context(|| format!("cannot create branch '{branch}'"))?;
    Ok(())
}

/// Create a local branch pointing at an existing one
/// (e.g. 'tod' from 'origin/tod' after a fresh clone).
pub fn create_branch_from(root: &Path, branch: &str, from: &str) -> Result<()> {
    git(root, &["branch", branch, from])
        .with_context(|| format!("cannot create branch '{branch}' from '{from}'"))?;
    Ok(())
}

/// Check out 'branch' in the worktree at 'path', creating it.
pub fn worktree_add(root: &Path, path: &Path, branch: &str) -> Result<()> {
    git(
        root,
        &[
            "worktree",
            "add",
            &path.to_string_lossy(),
            branch,
        ],
    )
    .with_context(|| format!("cannot check out branch '{branch}' in {}", path.display()))?;
    Ok(())
}

/// Create a branch in the user's repository and switch to it.
/// Fails when the branch already exists. `git checkout -b` alone is
/// not enough: it silently succeeds when the branch is the current
/// one, and a repo without any commit has unborn branches that do
/// not exist as refs yet.
pub fn create_branch(root: &Path, name: &str) -> Result<()> {
    let is_current = current_branch_in(root).map(|b| b == name).unwrap_or(false);
    if branch_exists(root, name)? || is_current {
        bail!("branch '{name}' already exists");
    }
    git(root, &["checkout", "-b", name])
        .with_context(|| format!("cannot create branch '{name}'"))?;
    Ok(())
}

/// Turn a task description into a valid git branch name: lowercase,
/// words joined with '-', at most 60 characters.
pub fn branch_slug(description: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in description.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() && slug.chars().count() < 60 {
            slug.push(c);
            pending_dash = false;
        } else if !slug.is_empty() && !pending_dash {
            slug.push('-');
            pending_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// Forget the worktrees whose directories have disappeared.
pub fn worktree_prune(root: &Path) -> Result<()> {
    git(root, &["worktree", "prune"]).context("git worktree prune failed")?;
    Ok(())
}

/// The remotes of the repository (shared across worktrees).
pub fn remotes(root: &Path) -> Result<Vec<String>> {
    Ok(git(root, &["remote"])?
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Fetch a remote. Fails when it cannot be reached.
pub fn fetch(dir: &Path, remote: &str) -> Result<()> {
    git(dir, &["fetch", "--quiet", remote])
        .map(|_| ())
        .with_context(|| format!("cannot fetch '{remote}'"))
}

/// Fast-forward the branch checked out in 'dir' to 'from'.
/// Fails when the histories diverged.
pub fn merge_ff_only(dir: &Path, from: &str) -> Result<()> {
    git(dir, &["merge", "--ff-only", from])
        .map(|_| ())
        .with_context(|| format!("cannot fast-forward to '{from}'"))
}

/// Push a branch to a remote, creating it there if needed.
pub fn push(dir: &Path, remote: &str, branch: &str) -> Result<()> {
    git(dir, &["push", "--quiet", remote, branch])
        .map(|_| ())
        .with_context(|| format!("cannot push '{branch}' to '{remote}'"))
}

/// Start a push in the background and return immediately: the
/// command goes on (or the process exits) while git keeps running.
/// The child is reaped by a detached thread, so a long-lived server
/// process never accumulates zombies.
pub fn spawn_push(dir: &Path, remote: &str, branch: &str) -> Result<()> {
    let mut child = Command::new("git")
        .args(["push", "--quiet", remote, branch])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("cannot start the background push of '{branch}'"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// How many commits does 'branch' have that 'from' (e.g. a
/// remote-tracking ref) lacks? Purely local, no network involved.
pub fn ahead_of(dir: &Path, from: &str, branch: &str) -> Result<usize> {
    git(dir, &["rev-list", "--count", &format!("{from}..{branch}")])?
        .trim()
        .parse()
        .with_context(|| format!("cannot compare '{branch}' with '{from}'"))
}

/// Stage every change of 'dir' and commit it. Returns whether a
/// commit was actually created (false when there was nothing to
/// commit). The committer identity is forced to 'tod' so the
/// storage branch works even in repositories without user.name set.
pub fn commit_all(dir: &Path, message: &str) -> Result<bool> {
    git(dir, &["add", "-A"]).context("git add failed")?;
    let staged = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(dir)
        .output()
        .context("failed to run git")?;
    if staged.status.success() {
        return Ok(false); // nothing to commit
    }
    git(
        dir,
        &[
            "-c",
            "user.name=tod",
            "-c",
            "user.email=tod@localhost",
            "commit",
            "-m",
            message,
        ],
    )
    .with_context(|| format!("cannot commit: {message}"))?;
    Ok(true)
}

/// Current git branch of the repository rooted at 'dir': the "context" of dot.
pub fn current_branch_in(dir: &Path) -> Result<String> {
    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir)
        .output()
        .context("failed to run git")?;
    let branch = String::from_utf8(out.stdout)?.trim().to_string();
    if branch.is_empty() {
        bail!("detached HEAD state: no current branch");
    }
    Ok(branch)
}

/// Current git branch of the current working directory.
pub fn current_branch() -> Result<String> {
    current_branch_in(&std::env::current_dir().context("cannot get current directory")?)
}

/// Git user name, used as task author.
pub fn user_name() -> String {
    Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_slug_is_a_valid_branch_name() {
        assert_eq!(
            branch_slug("Créer une commande qui start une tache"),
            "créer-une-commande-qui-start-une-tache"
        );
        assert_eq!(branch_slug("Fix the login bug!"), "fix-the-login-bug");
        assert_eq!(branch_slug("  spaces  &  symbols  "), "spaces-symbols");
        assert_eq!(branch_slug("###"), "");
        assert_eq!(branch_slug(&"x".repeat(100)).chars().count(), 60);
    }
}
