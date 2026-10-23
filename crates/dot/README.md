# dot — the library behind tod

The Rust port of the "dot" library of
[tod-cli](https://github.com/timber-dev-society/tod-cli), grown into
the full storage engine of the [tod CLI](../cli): a tiny schema-based
document format, the task model, and a git-native todo store with
its planning tools.

## Modules

| module | role |
|---|---|
| `schema` | the dot document format: `Key: value` lines, tab-indented blocks, parsing/stringifying |
| `types` | the field types: `Text`, `Bool` (0/1), `Iso8601` |
| `task` | `Task` + its dot schema (`task.tod`), tag normalization |
| `store` | the git-native store: states, sync, mutations |
| `git` | the git helpers: worktrees, branches, rebase, push, retrying background push |
| `plan` | `plan.md`: the micro-todo checklist of a task |
| `spec` | `spec.md`: the authored detailed prompt (scaffold + read) |
| `scenario` | `scenario.feature`: the Gherkin acceptance criteria |
| `brief` | — removed: specs are authored, never generated |

## The storage model

The todos live in a dedicated `tod` git branch, checked out in a
private worktree inside `.git/` (never in the user's working
copy). One directory per task, named by its sha1 uid, inside the
directory of its **state**:

```
backlog/<sha1>/task.tod   # the task itself (dot format)
todo/<sha1>/              # one folder per state, in lifecycle
review/<sha1>/            # order: backlog, todo, review, test,
test/<sha1>/              # done, canceled
done/<sha1>/
canceled/<sha1>/
```

The **state** of a task is its directory (`Store::STATES`); the
**working branch** is a `Branch:` field of `task.tod`. Each task
directory also holds `plan.md` (a markdown checklist), `spec.md`
(the detailed prompt) and optionally `scenario.feature`.

## Synchronization

`Store::sync_pull` runs before any command: fetch origin,
fast-forward the tod branch, and — for a mirror (the server) —
fast-forward from `FETCH_HEAD` and resync the worktree. The push
runs at the end of each command, in the background by default,
with a retrying child (`git::spawn_push`); `commit` marks the
store dirty and `Drop` flushes once per command. Environment:
`TOD_SYNC_TTL` (pull rate limit), `TOD_SYNC_PUSH=sync`,
`TOD_NO_SYNC=1`.

## Key entry points

- `Store::discover()` — CLI: the tod branch of the current repo.
- `Store::new(root)` — explicit root (the server opens mirrors).
- `find_task` / `set_state` / `start_task` / `complete` /
  `ready_for_review` / `set_tags` / `resolve` / `complete_context`.
- `Plan::load` / `Plan::save` — the plan of a task directory.
- `spec::scaffold` / `scenario::scaffold` — the prompt skeletons.

Run the tests: `cargo test -p dot` (unit + integration, real
temporary git repositories per test).
