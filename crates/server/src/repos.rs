//! Clone / update the managed git repositories.
//!
//! Every registered repository lives as a bare mirror in the server
//! workdir. The repository registry is the SeaORM `repos` entity.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::models::repos::Model as Repo;

fn git(dir: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let out = cmd.output().context("failed to run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

/// The repository must be usable for tod: the 'tod' branch exists
/// (locally, or on origin after a fresh clone) and its persistent
/// worktree can be checked out.
fn require_tod(root: &Path) -> Result<()> {
    dot::store::Store::new(root)
        .ensure_init()
        .with_context(|| format!("{} is not initialized for tod", root.display()))
}

/// Clone (or update) a **bare mirror** of the repository 'repo' into
/// the server workdir, and return its path. Every registered
/// repository — remote URL or local path — is mirrored the same
/// way: the server always serves and mutates its own copy, never the
/// original. A mirror keeps every ref (branches, tags, the tod
/// branch) and accepts pushes on any of them, which a plain clone
/// with a checked-out branch would refuse.
/// Does the repository have a remote to sync with? Repositories
/// created on the server have none: they live here, nothing to
/// mirror or relay.
fn has_origin(root: &Path) -> bool {
    git(Some(root), &["remote"])
        .map(|remotes| !remotes.trim().is_empty())
        .unwrap_or(false)
}

pub fn prepare(repo: &Repo, workdir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(workdir)?;
    let root = workdir.join(format!("{}-{}.git", repo.id, repo.name));
    if root.is_dir() {
        // created repositories have no origin to sync from
        if has_origin(&root) {
            git(Some(&root), &["remote", "update", "--prune"])?;
        }
    } else if repo.url.trim().is_empty() {
        // a repository created here: an empty bare repository with
        // an initial commit on both main and the tod branch, ready
        // to be cloned right away
        create_bare(&root)?;
    } else {
        git(
            None,
            &["clone", "--mirror", &repo.url, &root.to_string_lossy()],
        )?;
        // The tod branch is pinned by the server's worktree: git
        // refuses to fetch into a checked-out branch. Keep every
        // other ref in sync from origin; the tod branch only moves
        // through pushes and the server's own commands.
        git(
            Some(&root),
            &["config", "--add", "remote.origin.fetch", "^refs/heads/tod"],
        )?;
    }
    // Let clients push the tod branch although it is checked out
    // in the server's worktree: updateInstead refreshes that
    // worktree when the push lands (it is always clean, the
    // server commits everything it does).
    git(
        Some(&root),
        &["config", "receive.denyCurrentBranch", "updateInstead"],
    )?;
    require_tod(&root)?;
    Ok(root)
}

/// Create an empty bare repository with an initial commit shared by
/// `main` and the `tod` branch: like GitHub's "new repository",
/// immediately clonable — and immediately tod-usable.
pub fn create_bare(root: &Path) -> Result<()> {
    if root.exists() {
        bail!("{} already exists", root.display());
    }
    git(None, &["init", "--bare", &root.to_string_lossy()])?;
    let empty_tree = {
        let out = std::process::Command::new("git")
            .args(["mktree"])
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .output()
            .context("failed to run git mktree")?;
        if !out.status.success() {
            bail!("git mktree failed");
        }
        String::from_utf8(out.stdout)?.trim().to_string()
    };
    let commit = git(Some(root), &["commit-tree", &empty_tree, "-m", "tod: init"])?;
    git(Some(root), &["update-ref", "refs/heads/main", &commit])?;
    git(Some(root), &["update-ref", "refs/heads/tod", &commit])?;
    git(Some(root), &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    Ok(())
}

/// The changes a branch carries on top of a base branch, for the
/// console's task detail. The unified diff is truncated so a single
/// response stays a sane payload for the browser.
#[derive(Debug, serde::Serialize)]
pub struct BranchDiff {
    pub stat: String,
    pub diff: String,
    pub truncated: bool,
}

/// Size cap of the diff payload.
const DIFF_MAX_BYTES: usize = 256 * 1024;

/// Compare `branch` to `base` (both must exist): a per-file summary
/// and the unified diff, truncated at DIFF_MAX_BYTES.
pub fn diff(root: &Path, base: &str, branch: &str) -> Result<BranchDiff> {
    if !dot::git::branch_exists(root, branch)? {
        bail!("branch {branch} does not exist");
    }
    let range = format!("{base}...{branch}");
    let stat = git(Some(root), &["diff", "--stat", &range])?;
    let full = git(Some(root), &["diff", &range])?;
    let truncated = full.len() > DIFF_MAX_BYTES;
    let diff = if truncated {
        // never cut in the middle of a line or a character
        let mut end = DIFF_MAX_BYTES.min(full.len());
        while !full.is_char_boundary(end) {
            end -= 1;
        }
        let mut cut = full[..end].to_string();
        if let Some(idx) = cut.rfind('\n') {
            cut.truncate(idx + 1);
        }
        cut
    } else {
        full
    };
    Ok(BranchDiff { stat, diff, truncated })
}

/// Relay the mirror's refs to the repository's origin: everything a
/// client pushed to the server, or the server committed itself, is
/// forwarded so the original repository stays up to date. Mirror
/// clones carry `remote.origin.mirror=true`, so a bare `git push`
/// forwards every ref (explicit refspecs are refused in that mode).
/// The push is best-effort per ref — an origin that refuses one ref
/// (a local checkout, a diverged branch) is reported as a warning
/// without blocking the others.
pub fn relay(root: &Path) -> Result<()> {
    // created repositories live here: nothing to forward upstream
    if !has_origin(root) {
        return Ok(());
    }
    let out = Command::new("git")
        .args(["push", "origin"])
        .current_dir(root)
        .output()
        .context("failed to run git push")?;
    if !out.status.success() {
        eprintln!(
            "warning: cannot relay everything to origin: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dot::store::Store;
    use std::process::Command;

    /// A repository entity with fake timestamps.
    fn test_repo(id: i64, name: &str, url: String) -> Repo {
        let now = chrono::Utc::now();
        Repo {
            id,
            name: name.to_string(),
            url,
            created_at: now.into(),
            updated_at: now.into(),
        }
    }

    /// A temp directory with a real git repository, its tod storage
    /// initialized and one backlog task.
    fn git_repo_with_tod() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
        assert!(out.status.success(), "git init failed");
        let store = Store::new(dir.path());
        store.init().unwrap();
        store.add_in(true, "origin task", "tester").unwrap();
        dir
    }

    #[test]
    fn diff_shows_the_branch_changes() {
        let dir = tempfile::tempdir().unwrap();
        git(Some(dir.path()), &["init", "-b", "main"]).unwrap();
        git(Some(dir.path()), &["config", "user.email", "t@t"]).unwrap();
        git(Some(dir.path()), &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "one").unwrap();
        git(Some(dir.path()), &["add", "-A"]).unwrap();
        git(Some(dir.path()), &["commit", "-m", "init"]).unwrap();
        git(Some(dir.path()), &["checkout", "-b", "feat"]).unwrap();
        std::fs::write(dir.path().join("b.txt"), "two").unwrap();
        git(Some(dir.path()), &["add", "-A"]).unwrap();
        git(Some(dir.path()), &["commit", "-m", "feat"]).unwrap();

        let d = diff(dir.path(), "main", "feat").unwrap();
        assert!(d.stat.contains("b.txt"));
        assert!(d.diff.contains("+two"));
        assert!(!d.truncated);

        // an unknown branch is an error, not a panic
        assert!(diff(dir.path(), "main", "nope").is_err());
    }

    #[test]
    fn local_repo_without_tod_branch_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = test_repo(1, "p", dir.path().to_string_lossy().into_owned());
        let workdir = tempfile::tempdir().unwrap();
        assert!(prepare(&repo, workdir.path()).is_err());
    }

    #[test]
    fn prepare_mirrors_the_repository_and_serves_its_todos() {
        let origin = git_repo_with_tod();
        let workdir = tempfile::tempdir().unwrap();
        let repo = test_repo(7, "proj", origin.path().to_string_lossy().into_owned());

        let root = prepare(&repo, workdir.path()).unwrap();
        assert!(root.ends_with("7-proj.git"));
        assert_eq!(git(Some(&root), &["rev-parse", "--is-bare-repository"]).unwrap(), "true");

        // every ref travelled into the mirror, tod included
        let refs = git(Some(&root), &["for-each-ref", "--format=%(refname)", "refs/heads/*"]).unwrap();
        assert!(refs.contains("refs/heads/tod"));

        // the mirror carries its own tod worktree and reads the tasks
        let store = Store::new(&root);
        let tasks = store.load_backlog().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description, "origin task");
        assert!(root.join("tod-worktree").join(".git").is_file());

        // preparing again refreshes the mirror instead of re-cloning
        prepare(&repo, workdir.path()).unwrap();
    }

    #[test]
    fn relay_forwards_client_pushes_to_the_origin() {
        // a bare origin: like a real origin, nothing is checked out
        let origin_dir = tempfile::tempdir().unwrap();
        let origin = origin_dir.path().join("origin.git");
        let out = Command::new("git")
            .args(["init", "--bare", origin.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success(), "git init --bare failed");

        // a first user seeds the origin: code plus the tod storage
        let seed = tempfile::tempdir().unwrap();
        git(None, &["clone", origin.to_string_lossy().as_ref(), seed.path().to_string_lossy().as_ref()]).unwrap();
        git(Some(seed.path()), &["config", "user.email", "t@t"]).unwrap();
        git(Some(seed.path()), &["config", "user.name", "t"]).unwrap();
        std::fs::write(seed.path().join("app.txt"), "code").unwrap();
        git(Some(seed.path()), &["add", "-A"]).unwrap();
        git(Some(seed.path()), &["commit", "-m", "code"]).unwrap();
        git(Some(seed.path()), &["push", "origin", "main"]).unwrap();
        {
            let store = Store::new(seed.path());
            store.init().unwrap();
            store.add_in(true, "seed task", "tester").unwrap();
        } // dropped: the tod branch is pushed to the bare origin

        // the server mirrors the repository
        let workdir = tempfile::tempdir().unwrap();
        let repo = test_repo(3, "proj", origin.to_string_lossy().into_owned());
        let mirror = prepare(&repo, workdir.path()).unwrap();
        assert!(git(Some(&mirror), &["rev-parse", "--verify", "refs/heads/tod"]).is_ok());

        // a client pushes a feature branch to the server's mirror
        let client = tempfile::tempdir().unwrap();
        git(None, &["clone", mirror.to_string_lossy().as_ref(), client.path().to_string_lossy().as_ref()]).unwrap();
        git(Some(client.path()), &["config", "user.email", "t@t"]).unwrap();
        git(Some(client.path()), &["config", "user.name", "t"]).unwrap();
        std::fs::write(client.path().join("feature.txt"), "work").unwrap();
        git(Some(client.path()), &["add", "-A"]).unwrap();
        git(Some(client.path()), &["commit", "-m", "feature"]).unwrap();
        git(Some(client.path()), &["push", "origin", "main"]).unwrap();
        assert!(git(Some(&mirror), &["rev-parse", "--verify", "refs/heads/main"]).is_ok());

        // the client's commit is on the mirror, not yet on the origin
        let mirrored = git(Some(&mirror), &["rev-parse", "refs/heads/main"]).unwrap();
        let before = git(Some(&origin), &["rev-parse", "refs/heads/main"]).unwrap();
        assert_ne!(mirrored, before);

        // the relay forwards everything the client pushed to the origin
        relay(&mirror).unwrap();
        assert!(git(Some(&origin), &["rev-parse", "--verify", "refs/heads/main"]).is_ok());
        let relayed = git(Some(&origin), &["rev-parse", "refs/heads/main"]).unwrap();
        assert_eq!(mirrored, relayed);
    }
}
