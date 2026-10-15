//! The agent skill: a SKILL.md installed in the repository so that
//! an AI coding agent knows how to read and update the tod database.

use std::path::PathBuf;

use anyhow::{Context, Result};

use dot::store::Store;

/// Where the skill is installed, depending on the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    /// Claude Code
    Claude,
    /// Cursor
    Cursor,
    /// Mistral Vibe
    Vibe,
    /// Any other agent following the .agents convention
    Generic,
}

impl Agent {
    /// Directory of the skill inside the repository.
    pub fn skill_dir(self) -> &'static str {
        match self {
            Agent::Claude => ".claude/skills/tod",
            Agent::Cursor => ".cursor/skills/tod",
            Agent::Vibe => ".vibe/skills/tod",
            Agent::Generic => ".agents/skills/tod",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Cursor => "Cursor",
            Agent::Vibe => "Mistral Vibe",
            Agent::Generic => "other (.agents)",
        }
    }

    /// Parse a --agent flag value.
    pub fn from_flag(value: &str) -> Option<Agent> {
        match value.trim().to_lowercase().as_str() {
            "claude" | "claude-code" => Some(Agent::Claude),
            "cursor" => Some(Agent::Cursor),
            "vibe" | "mistral" => Some(Agent::Vibe),
            "agents" | "generic" | "other" => Some(Agent::Generic),
            _ => None,
        }
    }
}

pub fn skill_path(store: &Store, agent: Agent) -> PathBuf {
    store.root().join(agent.skill_dir()).join("SKILL.md")
}

/// The skill content. Kept in sync with the README.
pub const SKILL_MD: &str = r#"---
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
tod rm / mv / start / ctx / resolve / export   # (human) maintenance
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
"#;

/// Install (or overwrite) the skill for one agent.
/// Returns the SKILL.md path.
pub fn install(store: &Store, agent: Agent) -> Result<PathBuf> {
    let path = skill_path(store, agent);
    let dir = path.parent().expect("skill path has a parent");
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    std::fs::write(&path, SKILL_MD)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

/// Remove the skill of one agent. Ok(false) when it was not installed.
pub fn remove(store: &Store, agent: Agent) -> Result<bool> {
    let path = skill_path(store, agent);
    if !path.is_file() {
        return Ok(false);
    }
    std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
    // drop the folder too when it is now empty
    let _ = std::fs::remove_dir(path.parent().expect("skill path has a parent"));
    Ok(true)
}

/// Remove every installed skill (all agents). Returns how many were removed.
pub fn remove_all(store: &Store) -> Result<usize> {
    let mut removed = 0;
    for agent in [Agent::Claude, Agent::Cursor, Agent::Vibe, Agent::Generic] {
        if remove(store, agent)? {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_then_remove_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let path = install(&store, Agent::Claude).unwrap();
        assert!(path.is_file());
        assert!(path.ends_with(".claude/skills/tod/SKILL.md"));
        assert!(std::fs::read_to_string(&path).unwrap().contains("# tod"));
        assert!(remove(&store, Agent::Claude).unwrap());
        assert!(!path.exists());
        assert!(!remove(&store, Agent::Claude).unwrap()); // already gone
    }

    #[test]
    fn each_agent_gets_its_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        for agent in [Agent::Claude, Agent::Cursor, Agent::Vibe, Agent::Generic] {
            let path = install(&store, agent).unwrap();
            assert!(path.to_string_lossy().starts_with(dir.path().to_string_lossy().as_ref()));
            assert!(path.ends_with("tod/SKILL.md"));
            assert!(path.to_string_lossy().contains(match agent {
                Agent::Claude => ".claude",
                Agent::Cursor => ".cursor",
                Agent::Vibe => ".vibe",
                Agent::Generic => ".agents",
            }));
        }
        assert_eq!(remove_all(&store).unwrap(), 4);
    }

    #[test]
    fn flag_parsing() {
        assert_eq!(Agent::from_flag("claude"), Some(Agent::Claude));
        assert_eq!(Agent::from_flag("Claude-Code"), Some(Agent::Claude));
        assert_eq!(Agent::from_flag("cursor"), Some(Agent::Cursor));
        assert_eq!(Agent::from_flag("generic"), Some(Agent::Generic));
        assert_eq!(Agent::from_flag("nonsense"), None);
    }
}
