# tod-server — the multi-repo project management server

A [Loco](https://loco.rs) application (axum, SeaORM, SQLite) that
manages several git repositories at once: login, repository
registry, git mirrors, git-over-SSH, and the JSON API behind the
[web console](../web). Binary name: `tod-server`.

## Run it

```bash
cargo run -p server -- start            # development (config/)
cargo run -p server -- db migrate       # apply the migrations
cargo run -p server -- task user:create \
    email:you@example.com name:You password:secret   # first user
```

The `settings` section of the config drives the tod specifics:
`workdir` (mirrors + SSH host key), `ssh_port` (default 2222),
`static_dir` (the built web console). Production ships its own
config (`--environment production`).

## Architecture

| layer | what |
|---|---|
| `models/` | `users` (JWT + password hash), `repos`, `user_keys` |
| `controllers/` | `auth` (login/register), `account` (profile, password), `keys` (SSH keys), `repos` (status, tasks, diff) |
| `views/` | the JSON contracts shared with the console: `TaskView` (state, branch, tags, plan, spec, scenario), `RepoView` (with `clone_url`) |
| `repos.rs` | the git mirrors: clone/update, bare creation, diff |
| `ssh.rs` | the embedded SSH server: keys, git-upload-pack/receive-pack only |

Each registered repository is cloned as a **bare mirror** in the
workdir and refreshed before each use — the original is never
touched. Repositories created on the server (`url: ""`) are born
bare and tod-ready. The tod branch of each mirror is synced from
origin on demand (FETCH_HEAD fast-forward + worktree resync).

## API (bearer token required)

- `POST /api/login {username, password}` → `{token}`
- `GET /api/account` — the logged-in user
- `POST /api/account/password {current, new}`
- `GET/POST /api/keys`, `DELETE /api/keys/:id` — the SSH keys
- `GET /api/repos` — with the SSH `clone_url` of each repository
- `POST /api/repos {name, url}` — register / create (`url: ""`)
- `DELETE /api/repos/:id`
- `GET /api/repos/:id/status` — the kanban: every state with its
  tasks (plan, progress, spec, scenarios, branch, tags)
- `GET /api/repos/:id/diff?branch=&base=` — the code changes of a
  branch, for the console's task detail (256 KiB cap)
- `POST /api/repos/:id/task {description}` — new task in the backlog
- `POST /api/repos/:id/sub {uid, text}` / `.../check {uid, number}`
- `POST /api/repos/:id/tag {uid, tags}` / `.../state {uid, state}`

Mutations are committed to the repository's tod branch and pushed
to its origin (async, retrying).

## Git over SSH

```bash
git clone ssh://git@server:2222/<repo-name>.git
```

The login name is ignored: the SSH key identifies the user. Only
`git-upload-pack` and `git-receive-pack` run; pushes are relayed to
the repository's origin.

Tests: `cargo test -p server` (mirrors, SSH, diff — real
repositories in temp directories).
