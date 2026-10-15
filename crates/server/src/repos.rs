//! Clone / update the managed git repositories.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::db::Repo;

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
pub fn prepare(repo: &Repo, workdir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(workdir)?;
    let root = workdir.join(format!("{}-{}.git", repo.id, repo.name));
    if root.is_dir() {
        git(Some(&root), &["remote", "update", "--prune"])?;
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
        // Let clients push the tod branch although it is checked out
        // in the server's worktree: updateInstead refreshes that
        // worktree when the push lands (it is always clean, the
        // server commits everything it does).
        git(
            Some(&root),
            &["config", "receive.denyCurrentBranch", "updateInstead"],
        )?;
    }
    require_tod(&root)?;
    Ok(root)
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

    /// A temp directory with a real git repository, its tod storage
    /// initialized and one backlog task.
    fn git_repo_with_tod() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
        assert!(out.status.success(), "git init failed");
        let store = Store::new(dir.path());
        store.init().unwrap();
        store.add_in(None, "origin task", "tester").unwrap();
        dir
    }

    #[test]
    fn local_repo_without_tod_branch_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo { id: 1, name: "p".into(), url: dir.path().to_string_lossy().into_owned() };
        let workdir = tempfile::tempdir().unwrap();
        assert!(prepare(&repo, workdir.path()).is_err());
    }

    #[test]
    fn prepare_mirrors_the_repository_and_serves_its_todos() {
        let origin = git_repo_with_tod();
        let workdir = tempfile::tempdir().unwrap();
        let repo = Repo { id: 7, name: "proj".into(), url: origin.path().to_string_lossy().into_owned() };

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
            store.add_in(None, "seed task", "tester").unwrap();
        } // dropped: the tod branch is pushed to the bare origin

        // the server mirrors the repository
        let workdir = tempfile::tempdir().unwrap();
        let repo = Repo { id: 3, name: "proj".into(), url: origin.to_string_lossy().into_owned() };
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
