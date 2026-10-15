---
name: tod
description: Manage the todos, plans and briefs of this repository with the tod CLI. Use it before starting work (pick the next step), and after completing work (check the step, read the brief).
---

# tod — git-native todo & planning for this repository

This repository tracks its work with **tod**: tasks live in a
dedicated `tod` git branch (checked out in a hidden worktree, never
in your working copy) — one plain-text file per task, a central plan
document `plan.dot`, and agent briefs in `md/<uid>.md`.

## Concepts

- A git **branch is a context**: tasks live per branch, plus a
  shared **backlog**.
- Each task has a **sha1 uid**; you can abbreviate it, git-style
  (`919fe` for `919fea...`).
- Complex tasks have a **plan**: a numbered list of micro todos.
- The **brief** (`tod brief <id>`) is the up-to-date spec markdown
  of a task: objective, context, status, plan with progress.

## Workflow for an agent

1. **Pick work**: run `tod status` (overview) or `tod ls` (current
   context). Choose the next task, or ask the human which one.
2. **Read the brief**: `tod brief <id>` — it is the source of truth
   for what the task is about.
3. **Execute the next undone step** of the plan, exactly one
   micro-todo at a time. Do not skip steps; if a step is wrong or
   blocked, say so instead of marking it done.
4. **Mark the step done**: `tod check <id> <number>` (1-based, as
   shown by status). Only check steps you actually completed.
5. **When a step reveals new work**, propose it to the human; the
   human adds it with `tod sub <id> "text"`. Never rewrite
   `plan.dot` by hand.
6. **When the whole task is done**, ask the human to run
   `tod x <id>`; do not complete a task on your own.

## Commands

```bash
tod status              # backlog + contexts, with [done total] progress
tod ls / ls -b          # todos of the current context / of the backlog
tod brief <id>          # the agent-facing spec markdown of a task
tod check <id> <n>      # toggle step n of a task's plan
tod add "text"          # (human) new todo in the current context
tod plan "text"         # (human) new task with a plan + brief
tod sub <id> "text"     # (human) add a step to a plan
tod x <id>              # (human) mark a task complete
tod rm / mv / ctx / resolve / export   # (human) maintenance
```

## Storage (read-only for agents)

The todos live in the `tod` git branch, checked out in a private
worktree inside `.git/` — never in your working copy.

```
todo/<branch>/<sha1>      # task files (format dot) — do not hand-edit
backlog/<sha1>            # backlog tasks — do not hand-edit
plan.dot                  # every plan, one document — do not hand-edit
md/<sha1>.md              # generated briefs — do not hand-edit
```

Agents interact with tod **only through the CLI**. The files above
are the human's storage; the briefs in `md/` are generated.

## Rules

- Never mark a step or task done unless you finished it.
- One step at a time: small, verifiable changes.
- If the plan is stale (the code moved on), tell the human instead
  of improvising changes to the plan.
