# tod — the CLI

The command line of [tod-do](../../README.md), built with clap on
the [dot](../dot) crate. Binary name: `tod`.

## The lifecycle it drives

```
backlog -> todo -> review -> test -> done
                \-> canceled

main ──┬──────────────────────────────────── merge (finish)
       └─ t/<task> ── merge (tod test) ─→ test branch
```

- `tod start <id>` — create the working branch from `main`
  (`t/<uid7>-<slug>`), move the task to `todo`.
- `tod rfr <id>` — review state + **blocking push** of the branch
  to the sync remote (the console diff reads it).
- `tod test <id>` — test state + merge into the `test` branch.
- `tod finish` — rebase, ff-merge into main, tasks done, branch
  deleted, test branch rebased clean.
- `tod x` / `tod state` / `tod rm` — done, any state, canceled.
- `tod verify` — run the validation tests of the project (see
  below); the command's exit code is tod's.

## The project configuration: `.tod.toml`

The per-repository settings, at the root — scaffolded by
`tod init` when missing, never overwritten afterwards:

```toml
[sync]
remote = "origin"       # the remote the 'tod' branch syncs with,
                        # and where rfr/test push the branches
[verify]
command = "cargo test"  # run by `tod verify` at the root,
                        # through the shell
```

Every key is optional: a missing file means the defaults
(`origin`, no verification). A broken file or an unknown key
fails the commands loudly instead of being silently ignored.
When `[sync] remote` names anything but `origin`, the whole sync
flow — pull before each command, push after each mutation,
adoption after a clone — goes through that remote.

## Working on a task

```bash
tod plan "Fix the login bug"   # task + plan + spec scaffold
tod spec 919fe -e              # write the detailed prompt
tod scenario 919fe -e         # write the Gherkin criteria
tod sub 919fe "Next step"      # add a micro todo
tod check 919fe 1              # toggle it
tod brief 919fe                # read the spec
```

The spec (`spec.md`) is the **detailed prompt** — analysis of the
existing code, research, reflections, work to realize — authored
by the human and the agent, never generated. `-e` opens it in
`$TOD_EDITOR`, then `$EDITOR`, then `vi` (flags allowed:
`TOD_EDITOR="code -w"`), and commits on exit.

## Listing

- `tod status` — every state, progress, branch and tags.
- `tod ls` — the current branch; `-b` the backlog; `-A` every
  state; `-a` includes done/canceled; `-t <tag>` filters.

## The agent skill

`tod init` / `tod skill` installs a `SKILL.md` teaching your AI
coding agent the workflow (pick work, read the spec and the
scenarios, one micro-todo at a time, `tod check`, never regenerate
the spec). Per-agent folders: Claude Code, Cursor, Mistral Vibe,
standard `.agents`.

## The agent hooks

`tod hooks` installs the deterministic layer besides the skill:
the context of the branch injected at session start (and after
each compaction), and the reminder to check the plan steps at the
end of a turn. Claude Code (`.claude/settings.json`: SessionStart
blocks nothing and feeds stdout to the agent, Stop exits 2 to
block, `stop_hook_active` guards the loop), Vibe (`.vibe/hooks.toml`:
`post_agent` with a structured JSON deny, capped at 3 per turn).
Cursor and the generic agent have no hook format: clean refusal.

## Layout

- `main.rs` — the clap commands, dynamic shell completion.
- `commands.rs` — every command, the editor helper, and the
  workflow tests (`start_in`, `test_in`, `finish_in`).
- `skill.rs` — the installable SKILL.md content.
- `hooks.rs` — the agent hooks: `context` (the snapshot), `reminder`
  (Stop / post_agent protocols), the installers and their tests.

Build: `cargo build -p cli` (or `cargo install --path crates/cli`).
Tests: `cargo test -p cli`.
