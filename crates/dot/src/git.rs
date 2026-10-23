use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// The integration branch of the tasks in the test state:
/// parallel to `main`, accumulating their work so a tester
/// checks out one branch and tries everything.
pub const TEST_BRANCH: &str = "test";

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

/// Switch the working tree to a branch.
pub fn checkout(root: &Path, branch: &str) -> Result<()> {
    git(root, &["checkout", branch])
        .with_context(|| format!("cannot check out '{branch}'"))
        .map(|_| ())
}

/// Rebase the current branch onto 'base'. Conflicts stop the
/// rebase; the user resolves them and runs the command again.
pub fn rebase(dir: &Path, base: &str) -> Result<()> {
    git(dir, &["rebase", base])
        .with_context(|| format!("cannot rebase onto '{base}' (resolve the conflicts, then re-run)"))
        .map(|_| ())
}

/// Merge a branch into the branch checked out in 'dir'. Conflicts
/// stop the merge: resolve them, commit, and run the command again.
pub fn merge(dir: &Path, branch: &str) -> Result<()> {
    git(dir, &["merge", "--no-edit", branch])
        .with_context(|| format!("cannot merge '{branch}' (resolve the conflicts, commit, and re-run)"))
        .map(|_| ())
}

/// Delete a branch, but only when it is fully merged.
pub fn delete_branch(root: &Path, branch: &str) -> Result<()> {
    git(root, &["branch", "-d", branch])
        .with_context(|| format!("cannot delete branch '{branch}'"))
        .map(|_| ())
}

/// Transliterate the common accented Latin characters to ASCII, so
/// branch names stay tool-friendly (URLs, shells, percent-encoding).
fn ascii_fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => "a",
        'è' | 'é' | 'ê' | 'ë' => "e",
        'ì' | 'í' | 'î' | 'ï' => "i",
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' => "o",
        'ù' | 'ú' | 'û' | 'ü' => "u",
        'ç' => "c",
        'ñ' => "n",
        'œ' => "oe",
        'æ' => "ae",
        'ß' => "ss",
        _ => return None,
    })
}

/// Turn a task description into the slug of its branch name: pure
/// ASCII (accents transliterated), words joined with '-', never cut
/// in the middle of a word, at most 30 characters — the task uid in
/// the branch name already identifies it, the slug is only a
/// readable hint.
pub fn branch_slug(description: &str) -> String {
    /// Max length of the slug.
    const MAX: usize = 30;

    // split into ASCII words
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for orig in description.chars() {
        let c = orig.to_lowercase().next().unwrap_or(orig);
        let folded: String = match ascii_fold(c) {
            Some(f) => f.to_string(),
            None if c.is_ascii_alphanumeric() => c.to_string(),
            None => String::new(),
        };
        if folded.is_empty() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push_str(&folded);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }

    // join as many whole words as fit
    let mut slug = String::new();
    for word in words {
        if slug.is_empty() && word.len() > MAX {
            // a single overlong word: keep its beginning as a hint
            slug.push_str(&word[..MAX]);
            break;
        }
        if slug.len() + word.len() + usize::from(!slug.is_empty()) > MAX {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word);
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

/// Fetch a branch into the scratch ref `refs/tod-sync`. A mirror
/// (the server) cannot fetch into its checked-out tod branch, and
/// no fetch may ever move the branch backward: the caller
/// fast-forwards from the scratch ref, nothing else. The
/// explicit destination keeps git from touching the checked-out
/// branch and needs no --update-head-ok.
pub fn fetch_to_scratch(dir: &Path, remote: &str, branch: &str) -> Result<()> {
    git(
        dir,
        &["fetch", "--quiet", remote, &format!("refs/heads/{branch}:refs/tod-sync")],
    )
        .map(|_| ())
        .with_context(|| format!("cannot fetch '{branch}' from '{remote}'"))
}

/// Fast-forward the branch checked out in 'dir' to 'from'.
/// Fails when the histories diverged.
pub fn merge_ff_only(dir: &Path, from: &str) -> Result<()> {
    git(dir, &["merge", "--ff-only", from])
        .map(|_| ())
        .with_context(|| format!("cannot fast-forward to '{from}'"))
}

/// Is this remote a mirror clone? A mirror refuses any refspec on
/// push: the push mirrors every ref instead.
fn is_mirror(dir: &Path, remote: &str) -> bool {
    let out = Command::new("git")
        .args(["config", "--bool", &format!("remote.{remote}.mirror")])
        .current_dir(dir)
        .output();
    matches!(out, Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
}

/// Push a branch to a remote, creating it there if needed. In a
/// mirror clone every ref is pushed — a refspec would be refused.
pub fn push(dir: &Path, remote: &str, branch: &str) -> Result<()> {
    let args: Vec<&str> = if is_mirror(dir, remote) {
        vec!["push", "--quiet", remote]
    } else {
        vec!["push", "--quiet", remote, branch]
    };
    git(dir, &args)
        .map(|_| ())
        .with_context(|| format!("cannot push '{branch}' to '{remote}'"))
}

/// Start a push in the background and return immediately: the
/// command goes on (or the process exits) while git keeps running.
/// Concurrent pushes of the same branch race on the remote and the
/// loser is rejected as non-fast-forward, so the child retries a
/// few times — the next attempt reads the fresh branch tip and
/// fast-forwards. The child is reaped by a detached thread, so a
/// long-lived server process never accumulates zombies.
pub fn spawn_push(dir: &Path, remote: &str, branch: &str) -> Result<()> {
    // a mirror refuses refspecs: push every ref instead
    let push_args = if is_mirror(dir, remote) {
        format!("{remote}")
    } else {
        format!("{remote} {branch}")
    };
    let script = format!(
        "i=0; while [ $i -lt 5 ]; do \
         git push --quiet {push_args} 2>/dev/null && exit 0; \
         i=$((i+1)); sleep 1; done; exit 1"
    );
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&script)
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
        // pure ASCII: accents transliterated
        assert_eq!(
            branch_slug("Créer une commande qui start une tache"),
            "creer-une-commande-qui-start"
        );
        assert_eq!(branch_slug("Affichage à la trello"), "affichage-a-la-trello");
        assert_eq!(branch_slug("Fix the login bug!"), "fix-the-login-bug");
        assert_eq!(branch_slug("  spaces  &  symbols  "), "spaces-symbols");
        assert_eq!(branch_slug("###"), "");
        // never longer than 30, and never cut mid-word
        assert_eq!(branch_slug(&"x".repeat(100)), "x".repeat(30));
        let long = branch_slug("un un un un un un un un un un un un un");
        assert!(long.chars().count() <= 30 && !long.ends_with('-'));
    }
}
