# tod-do

A git-native todo CLI, written in Rust. Rust port of the
[tod-cli](https://github.com/timber-dev-society/tod-cli) concept,
extended with **planning support for complex tasks and agentic
development**, an **agent skill**, plus a **multi-repo project
management server** (login, SQLite, Yew web console, Docker image).

## Main concepts

- A git branch is a **context**: switch branches and you switch todos.
- One task = one independent plain-text file, to avoid merge conflicts.
- Each task file is named by a **sha1 identifier**. Like git commits,
  you only need the first characters of an identifier to target a task.
- A **backlog** holds tasks that do not belong to any branch yet.
- Complex tasks get a **plan** (micro todo list) and a **spec
  markdown** (the brief you hand to a development agent).

## Planning & agentic workflow

```
tod plan "Fix the login bug"     # task + plan + spec markdown skeleton
tod sub 919fe "Investigate expiry"   # add a micro todo to the plan
tod check 919fe 1                    # toggle step 1
tod brief 919fe                      # (re)generate + print the brief
tod status                           # backlog & contexts with progress
```

The **brief** is a self-contained markdown (objective, context,
status, plan with progress) meant to be pasted into an AI coding
agent. The agent executes the next step, the human runs
`tod check`, and the brief always reflects the real state: the
plan is the communication protocol between human and agent.

## Agent skill

`tod init` can install a **skill** for your AI coding agent: a
`SKILL.md` that teaches the agent the tod workflow (pick the next
step, `tod brief`, `tod check`, one micro-todo at a time, never
touch the `tod` branch directly).

The install asks **which agent you use**, because each one looks in
its own folder:

| choice | agent | skill folder |
|---|---|---|
| 1 | Claude Code | `.claude/skills/tod/` |
| 2 | Cursor | `.cursor/skills/tod/` |
| 3 | Mistral Vibe | `.vibe/skills/tod/` |
| 4 | other / standard | `.agents/skills/tod/` |

```bash
tod init                        # asks, then asks which agent
tod init --skill                # install, asks the agent when interactive
tod init --agent claude         # install for Claude Code, no question
tod init --no-skill             # skip
tod skill                       # (re)install later (asks the agent)
tod skill --agent cursor        # (re)install for Cursor
tod skill --remove              # remove every installed skill
tod skill --remove --agent claude  # remove only the Claude Code one
```

Commit the skill folder so every checkout (and every agent working
on the repo) picks it up.

## Project management server

`tod-server` manages **several repositories** at once. It is a
[Loco](https://loco.rs) application (controllers, SeaORM models,
migrations, YAML configuration) built on axum:

- **SQLite database** through SeaORM migrations (`users`, `repos`,
  `user_keys`): apply them with
  `cargo run -p server -- db migrate`.
- **Login**: passwords are hashed (Argon2), authentication is **JWT**
  (`POST /api/login {username, password}` — the username is the
  email — returns a bearer token). Create users with the CLI task:
  `cargo run -p server -- task user:create email:you@example.com name:You password:secret`.
- **Git mirrors**: each registered repository (https/git@/ssh URL, or
  a local path) is cloned into the workdir as a **bare mirror** and
  updated with `git remote update --prune` before each use. The
  original is never touched. A repository must have a `tod` branch
  (run `tod init` in it first).
- **Git over SSH**: the server embeds an SSH server — clients clone
  and push through it, authenticated by the registered keys.
- **Web console** served as the static fallback.

```bash
cargo run -p server -- start   # development (see crates/server/config/)
```

The `settings` section of the configuration file drives the tod
specifics: `workdir` (mirrors + SSH host key), `ssh_port` (default
2222), `static_dir` (built web console). Paths are relative to the
process working directory; production ships its own config
(`--environment production`).

API (all task endpoints are scoped per repository and need the
`Authorization: Bearer <token>` header):

- `POST /api/login {username, password}` → `{token}`
- `GET /api/repos` — list managed repositories
- `POST /api/repos {name, url}` — register an existing repository (clone + validate)
- `POST /api/repos {name, url: ""}` — **create** a new repository here (like GitHub),
  immediately clonable over SSH and tod-ready
- `DELETE /api/repos/:id` — unregister (the clone stays on disk)
- `GET /api/repos/:id/status` — branch, backlog, contexts, plans, progress
- `POST /api/repos/:id/task {description, context}`
- `POST /api/repos/:id/sub {uid, text}`
- `POST /api/repos/:id/check {uid, number}`
- `GET/POST /api/keys` — list / register your SSH public keys
- `DELETE /api/keys/:id` — remove one of your keys

Mutations are committed to the `tod` branch and pushed to the
repository's origin automatically.

### Git over SSH

The server is a git server in its own right: repositories are served
from their mirror, every branch can be fetched and pushed, and what
clients push is relayed to the repository's origin.

```bash
# register your SSH key (one line, from ~/.ssh/id_ed25519.pub)
curl -X POST http://server:8080/api/keys \
     -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
     -d "{\"public_key\":\"$(cat ~/.ssh/id_ed25519.pub)\"}"

# then use it like any git remote
git clone ssh://git@server:2222/<repo-name>.git
git push origin tod
```

The login name is ignored (`git@…`): the SSH key itself identifies
the user. Only `git-upload-pack` and `git-receive-pack` can run —
nothing else is executable through SSH. Pushing the `tod` branch
updates the server's own task view immediately.

### Web console (Yew)

The console is a Yew 0.21 CSR app in `crates/web`, styled with
[daisyUI](https://daisyui.com) (vendored full CSS, no npm in the
build): login screen, then a header navbar and hash-based pages —
repository list (`#/`), dedicated repository creation page (`#/new`),
and a full task board per repository (`#/repo/{id}`) with clickable
plans. Build it once with [Trunk](https://trunkrs.dev):

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cd crates/web && trunk build  # produces crates/web/dist
```

The `web` crate is **not** part of the workspace default members: a
plain `cargo build` at the root only builds dot, cli and server.

## Docker

A multi-stage Dockerfile builds the server and the web console, then
packs everything into a minimal Alpine image:

```bash
docker build -t tod-server .

docker run -p 8080:8080 -p 2222:2222 \
  -v tod-data:/data \
  tod-server
```

- `/data` (volume): the SQLite database (`tod-server.db`), the SSH
  host key, and the mirrored repositories (`repos/`).
- Create the first user in the container:
  `docker exec <container> tod-server --environment production task user:create email:you@example.com name:You password:secret`.
- The first start creates the admin account from the environment
  variables; afterwards users come from the database.
- `git` is included in the image (clone/pull at runtime).

## Storage layout

The todos live in a dedicated **`tod` git branch**, never in your
working copy. `tod init` creates the branch (empty of any project
code) and checks it out in a private worktree inside `.git/`; every
mutating command commits to it, so the branch always matches the
last executed command.

```
tod branch
  plan.dot                  # central tracking: every plan, one document
  md/<sha1>.md              # generated spec markdowns, one per task
  todo/<branch>/<sha1>      # the tasks themselves (format dot)
  backlog/<sha1>            # tasks waiting for a context
```

`plan.dot` is the source of truth for steps and progress; markdowns
are regenerated by `tod brief` (and refreshed by `sub`/`check`
when they already exist).

Your todos are versioned **and synchronized**: when the repository
has an `origin` remote, every command pulls the `tod` branch before
running, and every mutating command pushes it after committing —
work on any machine, the todos follow.

Commands stay fast in three ways. The pull is rate-limited: the
fetch runs at most once every `TOD_SYNC_TTL` seconds (default 30).
The push runs **in the background**: the command returns instantly
while git keeps running — if the push fails (offline, rejected), the
next command detects the unpushed commits and retries. Set
`TOD_SYNC_PUSH=sync` for a blocking push, `TOD_SYNC_TTL=0` to pull
before every command, or `TOD_NO_SYNC=1` to skip the network
entirely. Offline, commands keep working on the local branch with a
warning. A fresh clone recovers the branch automatically, and
`git log tod` reads like a changelog of your work.

## Monorepo layout

```
crates/dot     the dot library: document format (schema, parser,
               stringifier, typed fields), Task, git helpers, Store,
               planning (plan, brief)
crates/cli     the CLI (clap) built on top of the dot crate,
               binary name: tod
crates/server  the multi-repo project management server (axum):
               login + SQLite, git clone/pull, JSON API, web console,
               binary name: tod-server
crates/web     the Yew (CSR) web console, built with Trunk
```

## Task file format

One file per task, named by its sha1 uid:

```text
Description: Fix the login bug
Author: ndious
Created: 2026-10-06 18:12:44 +0000
Updated: 2026-10-06 18:12:44 +0000
Done: 0
```

And the central plan document (`plan.dot`):

```text
919fea91dd3193458488c70d168c5515362cbada:
    [x] Investigate the session expiry
    [ ] Add refresh token to auth.rs
```

## Install

Requires Rust (stable) and a git repository.

```bash
cargo install --path crates/cli
```

## Usage

```bash
tod init                    # create the tod branch storage (+ skill)
tod skill / skill --remove  # install / remove the agent skill
tod add "Small fix"        # a plain todo
tod plan "Complex feature" # a task with a plan + spec markdown
tod sub 919fe "Next step"  # add a micro todo to a plan
tod check 919fe 1          # toggle a step
tod brief 919fe            # the agent-facing brief
tod status                 # project overview with progress
tod ls / ls -b             # todos left to do (-a: also the done ones)
tod ls -A                   # every context (same filters)
tod ls -t rust              # only the tasks carrying a tag
tod x 919fe                # mark task complete (from anywhere)
tod rfr 919fe              # ready for review + push the work branch
tod tag 919fe rust cli      # replace the tags (none clears them)
tod rm 919fe / rm -b       # delete (also cleans plan + brief)
tod mv 919fe / mv -b       # backlog <-> context
tod start 919fe            # work branch for a task (from main, --base to stack)
tod finish                 # rebase + ff-merge into main, delete branch, close context
tod ctx                    # contexts with todos
tod resolve feature/login  # close a context
tod export todos.json      # export the whole database
tod completions bash       # shell completion script (bash, zsh, fish…)
```

### Task lifecycle

```
plan/add -> start -> work -> rfr -> review (web console) -> finish
```

`start` creates the working branch **from `main`** — tasks are
independent by default; `--base <branch>` stacks one on another
when it must. The branch is named `t/<uid7>-<slug>`: namespaced
under `t/` (the `tod/` prefix is impossible — git forbids
`refs/heads/tod/*` while the `tod` storage branch exists), linked
to its task by the uid, with a pure-ASCII slug (accents
transliterated, at most 30 characters, never cut mid-word). `rfr`
marks the task ready for review and pushes its branch to origin
(the web console shows the diff of pushed branches). `finish`
rebases the branch on the base, merges it fast-forward only,
deletes the merged branch and resolves its context. `x` closes a
task from anywhere, whatever branch you are on.

## Differences with tod-cli

- Rust + clap instead of Node.js + commander/redux.
- The dot document library is a standalone crate, usable without the CLI.
- No redux: plain, explicit functions. Errors are reported, never swallowed.
- Storage in a dedicated `tod` git branch managed through a hidden
  worktree, instead of `.dot` files in the working tree (2026-10).
- New: the planning layer (plan.dot + spec markdowns) for complex
  tasks and agentic development.
- New: the installable agent skill (SKILL.md, per-agent folder).
- New: the multi-repo management server (axum, login, SQLite) and
  the Yew web console, shipped as an Alpine Docker image.
- Same base command set: init, add, ls, x, rm, mv, ctx, resolve, export.

## License

MIT
