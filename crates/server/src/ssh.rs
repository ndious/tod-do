//! The embedded SSH server: git access (fetch and push) for the
//! managed repositories, authenticated by user SSH keys registered
//! through the HTTP API.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use russh::keys::ssh_key;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, Config, Handler, Server as _};

use crate::api::SharedState;

/// The SSH host key lives in the server workdir.
fn host_key_path(workdir: &Path) -> PathBuf {
    workdir.join("ssh_host_key")
}

/// The canonical text of a public key: openssh format without the
/// comment, so that a registered key ("ssh-ed25519 AAAA… user@laptop")
/// and the key presented during authentication (no comment on the
/// wire) compare equal.
pub fn canonical_key(key: &ssh_key::PublicKey) -> Result<String> {
    let mut key = key.clone();
    key.set_comment("");
    Ok(key.to_openssh()?)
}

/// Load the SSH host key, generating it on first start. The key is
/// kept on disk so clients can pin the server's fingerprint.
pub fn host_key(workdir: &Path) -> Result<PrivateKey> {
    std::fs::create_dir_all(workdir)?;
    let path = host_key_path(workdir);
    if path.is_file() {
        let pem = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        PrivateKey::from_openssh(&pem)
            .with_context(|| format!("invalid host key in {}", path.display()))
    } else {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .context("cannot generate the SSH host key")?;
        let pem = key
            .to_openssh(ssh_key::LineEnding::default())
            .context("cannot encode the SSH host key")?;
        std::fs::write(&path, pem.as_bytes())
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(key)
    }
}

/// The SSH server configuration: host key and sensible timeouts.
pub fn config(workdir: &Path) -> Result<Arc<Config>> {
    Ok(Arc::new(Config {
        keys: vec![host_key(workdir)?],
        inactivity_timeout: Some(std::time::Duration::from_secs(600)),
        ..Default::default()
    }))
}

/// Serve git over SSH on an already-bound listener, so the caller
/// controls the port (and reports bind failures) itself.
pub async fn serve_on(
    state: SharedState,
    config: Arc<Config>,
    socket: tokio::net::TcpListener,
) -> std::io::Result<()> {
    let mut server = SshServer { state };
    server.run_on_socket(config, &socket).await
}

/// The SSH server: one session handler per connection.
struct SshServer {
    state: SharedState,
}

/// One SSH connection. Authentication is by registered public key
/// only — the key itself identifies the user, the requested login
/// name is ignored (the git convention, like GitHub: git@...).
/// After authentication, the only allowed command is the git
/// protocol: git-upload-pack (fetch) or git-receive-pack (push).
struct SshSession {
    state: SharedState,
    /// The owner of the accepted key, once authenticated.
    user: Option<String>,
    /// The channel of the running command.
    channel: Option<russh::Channel<russh::server::Msg>>,
    /// stdin of the git subprocess (client -> git).
    child_stdin: Option<tokio::process::ChildStdin>,
}

impl russh::server::Server for SshServer {
    type Handler = SshSession;

    fn new_client(&mut self, _addr: Option<std::net::SocketAddr>) -> SshSession {
        SshSession {
            state: self.state.clone(),
            user: None,
            channel: None,
            child_stdin: None,
        }
    }
}

/// Parse the command git asks us to run. Strictly limited to the
/// git protocol: `git-upload-pack '/<repo>.git'` or
/// `git-receive-pack '/<repo>.git'`. Returns (service, repo name).
fn parse_git_command(command: &str) -> Option<(&'static str, &str)> {
    let (service, path) = command.trim().split_once(' ')?;
    let service = match service {
        "git-upload-pack" => "upload-pack",
        "git-receive-pack" => "receive-pack",
        _ => return None,
    };
    let path = path.trim_matches('\'');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let name = path.trim_start_matches('/');
    if name.is_empty() || name.contains('/') || name.contains("..") {
        return None;
    }
    Some((service, name))
}

impl Handler for SshSession {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: &ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        let Ok(text) = canonical_key(key) else {
            return Ok(Auth::reject());
        };
        match self.state.db.key_owner(&text) {
            Ok(Some(owner)) => {
                self.user = Some(owner);
                Ok(Auth::Accept)
            }
            _ => Ok(Auth::reject()),
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.channel = Some(channel);
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel_id: russh::ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        let Some(channel) = self.channel.take() else {
            session.channel_failure(channel_id)?;
            return Ok(());
        };
        session.channel_success(channel_id)?;

        // only authenticated users, only the git protocol
        let command = String::from_utf8_lossy(data).to_string();
        let Some(user) = self.user.clone() else {
            let _ = channel.exit_status(127).await;
            let _ = channel.close().await;
            return Ok(());
        };
        let Some((service, repo_name)) = parse_git_command(&command) else {
            eprintln!("ssh: {user} tried to run: {command:?}");
            let _ = channel.exit_status(127).await;
            let _ = channel.close().await;
            return Ok(());
        };

        // resolve the repository and its mirror
        let mirror = match self
            .state
            .db
            .find_repo_by_name(repo_name)
            .and_then(|repo| match repo {
                Some(repo) => crate::repos::prepare(&repo, &self.state.workdir),
                None => Err(anyhow::anyhow!("no repository named '{repo_name}'")),
            }) {
            Ok(mirror) => mirror,
            Err(e) => {
                eprintln!("ssh: {user}: {e:#}");
                let _ = channel.exit_status(127).await;
                let _ = channel.close().await;
                return Ok(());
            }
        };

        // run the git protocol in the mirror
        let mut child = match tokio::process::Command::new("git")
            .args([service, mirror.to_string_lossy().as_ref()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                eprintln!("ssh: cannot run git {service}: {e}");
                let _ = channel.exit_status(127).await;
                let _ = channel.close().await;
                return Ok(());
            }
        };
        self.child_stdin = child.stdin.take();
        eprintln!("ssh: {user} runs git {service} on '{repo_name}'");

        // forward the git output back to the client
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let mut stdout_writer = channel.make_writer();
        let mut stderr_writer = channel.make_writer_ext(Some(1));
        tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stdout, &mut stdout_writer).await;
        });
        tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stderr, &mut stderr_writer).await;
        });

        // wait for git, report the exit code, then relay pushes
        let is_receive = service == "receive-pack";
        tokio::spawn(async move {
            let code = match child.wait().await {
                Ok(status) => status.code().unwrap_or(1),
                Err(_) => 1,
            };
            let _ = channel.exit_status(code as u32).await;
            let _ = channel.close().await;
            if is_receive && code == 0 {
                // forward what the client pushed to the repository's origin
                if let Err(e) = crate::repos::relay(&mirror) {
                    eprintln!("ssh: {e:#}");
                }
            }
        });
        Ok(())
    }

    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        data: &[u8],
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        use tokio::io::AsyncWriteExt;
        if let Some(stdin) = self.child_stdin.as_mut() {
            let _ = stdin.write_all(data).await;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        // the client is done sending: close git's stdin
        self.child_stdin = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::db::Db;
    use tokio::io::AsyncReadExt;

    fn state(dir: &Path) -> SharedState {
        Arc::new(AppState {
            db: Db::open(&dir.join("test.db")).unwrap(),
            workdir: dir.join("workdir"),
        })
    }

    #[test]
    fn host_key_is_generated_once_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let workdir = dir.path().join("workdir");
        let first = host_key(&workdir).unwrap();
        assert!(workdir.join("ssh_host_key").is_file());
        // the next start reloads the same key
        let second = host_key(&workdir).unwrap();
        assert_eq!(first.public_key(), second.public_key());
    }

    #[tokio::test]
    async fn server_presents_the_ssh_banner() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let config = config(&state.workdir).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_on(state, config, listener));

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut banner = [0u8; 32];
        let n = stream.read(&mut banner).await.unwrap();
        assert!(banner[..n].starts_with(b"SSH-2.0"));
    }

    /// A minimal russh client that accepts any host key.
    struct NoVerify;

    impl russh::client::Handler for NoVerify {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _key: &russh::keys::PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    /// Authenticate with the given private key; true when accepted.
    async fn try_auth(addr: std::net::SocketAddr, key: PrivateKey) -> bool {
        let config = Arc::new(russh::client::Config::default());
        let mut session = russh::client::connect(config, addr, NoVerify)
            .await
            .unwrap();
        let auth = session
            .authenticate_publickey(
                "git",
                russh::keys::PrivateKeyWithHashAlg::new(
                    Arc::new(key),
                    session.best_supported_rsa_hash().await.unwrap().flatten(),
                ),
            )
            .await
            .unwrap();
        let _ = session
            .disconnect(russh::Disconnect::ByApplication, "", "English")
            .await;
        auth.success()
    }

    #[tokio::test]
    async fn registered_keys_authenticate_and_others_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        state.db.create_user("alice", "pass").unwrap();

        // alice registers her key through the API path (db)
        let alice_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let public = alice_key.public_key().to_openssh().unwrap();
        state.db.add_key("alice", &public).unwrap();

        // a key nobody registered
        let stranger_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();

        let config = config(&state.workdir).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_on(state, config, listener));

        assert!(try_auth(addr, alice_key).await, "registered key rejected");
        assert!(!try_auth(addr, stranger_key).await, "unknown key accepted");
    }

    /// Run a git command against the server as the ssh client would.
    fn git(dir: &Path, args: &[&str], ssh_command: &str) -> std::process::Output {
        use std::process::Command;
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_SSH_COMMAND", ssh_command)
            .output()
            .expect("cannot run git");
        if !out.status.success() {
            panic!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        out
    }

    /// Wait until 'predicate' is true, polling for up to 10 seconds:
    /// the relay of a push runs in the background.
    fn eventually(predicate: impl Fn() -> bool) -> bool {
        for _ in 0..200 {
            if predicate() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    fn rev_parse(dir: &Path, reference: &str) -> Option<String> {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "--verify", reference])
            .current_dir(dir)
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
    }

    #[tokio::test]
    async fn git_clients_fetch_and_push_over_ssh() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let work = dir.path().join("work");

        // a bare origin: the real repository the server mirrors
        let origin = dir.path().join("origin.git");
        let out = std::process::Command::new("git")
            .args(["init", "--bare", origin.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(out.status.success());

        // a first user seeds it: code plus the tod storage
        let seed = dir.path().join("seed");
        std::process::Command::new("git")
            .args(["clone", origin.to_string_lossy().as_ref(), seed.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", seed.to_string_lossy().as_ref(), "config", "user.email", "t@t"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", seed.to_string_lossy().as_ref(), "config", "user.name", "t"])
            .output()
            .unwrap();
        std::fs::write(seed.join("app.txt"), "code").unwrap();
        git(&seed, &["add", "-A"], "");
        git(&seed, &["commit", "-m", "code"], "");
        git(&seed, &["push", "origin", "main"], "");
        {
            use dot::store::Store;
            let store = Store::new(&seed);
            store.init().unwrap();
            store.add_in(Some("main"), "seed task", "tester").unwrap();
        } // dropped: the tod branch is pushed to the bare origin

        // the server: user + key + registered repository
        state.db.create_user("alice", "pass").unwrap();
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        state
            .db
            .add_key("alice", &key.public_key().to_openssh().unwrap())
            .unwrap();
        state
            .db
            .add_repo("proj", origin.to_string_lossy().as_ref())
            .unwrap();

        let config = config(&state.workdir).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve_on(state.clone(), config, listener));

        // the git client: ssh with the registered key
        let keyfile = dir.path().join("id_ed25519");
        std::fs::write(&keyfile, key.to_openssh(ssh_key::LineEnding::default()).unwrap().as_bytes())
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&keyfile, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let ssh_command = format!(
            "ssh -i {} -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o IdentitiesOnly=yes -o BatchMode=yes",
            keyfile.display()
        );

        // fetch: clone the repository through the server
        let clone = work.join("clone");
        git(
            dir.path(),
            &[
                "clone",
                &format!("ssh://git@127.0.0.1:{port}/proj.git"),
                clone.to_string_lossy().as_ref(),
            ],
            &ssh_command,
        );
        assert!(clone.join("app.txt").is_file());
        // the tod branch travelled too
        git(&clone, &["fetch", "origin", "tod"], &ssh_command);
        assert!(rev_parse(&clone, "origin/tod").is_some());

        // push: a commit on main goes through the server, into the
        // mirror, then is relayed to the origin
        git(&clone, &["config", "user.email", "c@c"], "");
        git(&clone, &["config", "user.name", "c"], "");
        std::fs::write(clone.join("feature.txt"), "work").unwrap();
        git(&clone, &["add", "-A"], "");
        git(&clone, &["commit", "-m", "feature"], "");
        git(&clone, &["push", "origin", "main"], &ssh_command);

        let pushed = rev_parse(&clone, "refs/heads/main").unwrap();
        assert!(
            eventually(|| rev_parse(&origin, "refs/heads/main").as_deref() == Some(pushed.as_str())),
            "the push never reached the origin"
        );

        // push the tod branch: the server's own worktree follows
        git(&clone, &["checkout", "-q", "-b", "tod", "origin/tod"], "");
        std::fs::create_dir_all(clone.join("todo/main")).unwrap();
        std::fs::write(clone.join("todo/main/dead"), "Description: done").unwrap();
        git(&clone, &["add", "-A"], "");
        git(&clone, &["commit", "-m", "tod: add dead"], "");
        git(&clone, &["push", "origin", "tod"], &ssh_command);

        let mirror = state.workdir.join("1-proj.git");
        assert!(
            eventually(|| mirror.join("tod-worktree/todo/main/dead").is_file()),
            "the server's tod worktree did not follow the push"
        );
    }
}
