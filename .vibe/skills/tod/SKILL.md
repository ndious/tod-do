---
name: tod
description: Manage the todos, plans and specs of this repository with the tod CLI. Use it before starting work (pick the next task, read its spec and scenarios), and while working (check the steps of the plan).
---

# tod — git-native todo & planning for this repository

This repository tracks its work with **tod**: the tasks live in a
dedicated `tod` git branch (checked out in a hidden worktree, never
in your working copy), one directory per task holding its files.

## Concepts

- A task has a **state** — backlog, todo, review, test, done,
  canceled — which is the directory holding it.
- Each task has a **sha1 uid**; abbreviate it git-style (`919fe`).
- A task in a working branch carries its **branch** name.
- A complex task has a **plan**: `plan.md`, a markdown checklist.
- The **spec** (`spec.md`) is the detailed prompt of the task:
  analysis of the existing code, research, reflections, and the
  work to realize. It is **authored** (by the human and you), never
  generated: fill it in, do not expect boilerplate.
- The **scenarios** (`scenario.feature`) are the acceptance
  criteria, in French Gherkin: convert them into tests of the
  project's own framework — they are the definition of done.

## Workflow for an agent

1. **Pick work**: `tod status` (overview by state) or `tod ls`
   (your current branch). Choose the next task, or ask the human.
2. **Read the spec**: `tod brief <id>` — the detailed prompt. Read
   the scenarios too: `tod scenario <id>`.
3. **Execute the next undone step** of the plan, exactly one micro
   todo at a time. If a step needs analysis or decisions, write
   them into the spec (`tod spec <id> -e`): research, reflections
   and analysis of the existing code belong there.
4. **Mark the step done**: `tod check <id> <number>` (1-based).
   Only check steps you actually completed.
5. **When a step reveals new work**, propose it to the human; the
   human adds it with `tod sub <id> "text"`.
6. **When the whole task is done**, ask the human to run
   `tod x <id>` (or `tod rfr <id>` when the work should be
   reviewed in the web console first).

## Commands

```bash
tod status              # every state, with progress
tod ls / ls -b / ls -A  # todos left to do (-a: the done ones too)
tod brief <id>          # the spec of a task — its detailed prompt
tod spec <id> -e        # edit the spec in $TOD_EDITOR / $EDITOR
tod scenario <id> [-e]  # the Gherkin scenarios (edit them too)
tod check <id> <n>      # toggle step n of the plan
tod sub <id> "text"     # (human) add a step to a plan
tod add / plan "text"   # (human) new todo / new task with a spec scaffold
tod x <id>              # (human) task done
tod rfr / test <id>     # (human) ready for review / merge into the test branch
tod start <id>          # (human) working branch from main, state todo
tod finish              # (human) rebase + ff-merge into main, cleanup
tod tag <id> t1 t2      # (human) replace the tags
tod completions <shell> # (human) shell completion (task ids included)
```

## Storage (read-only for agents)

The tasks live in the `tod` git branch, checked out in a private
worktree inside `.git/` — never in your working copy.

```
backlog/<sha1>/task.tod   # one directory per task
todo/<sha1>/              # ... one folder per state:
review/<sha1>/            # backlog, todo, review, test, done, canceled
test/<sha1>/
done/<sha1>/
canceled/<sha1>/
```

Each task directory holds `task.tod` (description, author, dates,
branch, tags), `plan.md` (the checklist you execute), `spec.md`
(the detailed prompt) and optionally `scenario.feature`. Agents
interact with tod **only through the CLI**; the spec and the
scenarios are written content: fill them, never regenerate them.

## Hooks (when installed)

`tod hooks` installs the deterministic counterpart of this skill in
your agent's own hook config (Claude Code: `.claude/settings.json`,
Vibe: `.vibe/hooks.toml`):

- **Context injection** — at session start, and again after each
  compaction, the state of the current branch arrives by itself:
  the active tasks and the next step of each plan. Read it before
  anything else; spare your context the `tod status` round trip.
- **Step reminder** — at the end of a turn, when a task of the
  branch still holds unchecked plan steps, you are reminded:
  check the steps you completed (`tod check <id> <n>`) or continue
  the plan. Claude Code blocks the stop until you do; Vibe nudges
  the turn. Sessions on `main` are never blocked.

## Rules

- Never mark a step or task done unless you finished it.
- One step at a time: small, verifiable changes.
- If the plan is stale (the code moved on), tell the human instead
  of improvising changes to the plan.
- Never regenerate or overwrite the spec and the scenarios: they
  are written content — extend them, do not replace them.

