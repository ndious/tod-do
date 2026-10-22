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

## Rules

- Never mark a step or task done unless you finished it.
- One step at a time: small, verifiable changes.
- If the plan is stale (the code moved on), tell the human instead
  of improvising changes to the plan.
- Never regenerate or overwrite the spec and the scenarios: they
  are written content — extend them, do not replace them.

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
