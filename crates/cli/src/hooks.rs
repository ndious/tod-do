//! The agent hooks: the **deterministic** layer installed in the
//! config files of the AI agent, complement of the skill — the
//! skill tells the agent what to do, the hooks make the context
//! arrive and the reminder fire no matter what.
//!
//! Two machine commands are called by the agent's hook config:
//!
//! - `tod hooks context` — the compact state of the working
//!   branch, injected at session start (and after each compaction:
//!   Claude Code's SessionStart matcher `compact` covers it).
//! - `tod hooks reminder` — reads the hook payload on stdin, and
//!   speaks the protocol of the calling agent: Claude Code's
//!   `Stop` (exit 2 blocks the stop, the message goes to stderr)
//!   or Vibe's `post_agent` (a structured JSON deny on stdout).

use std::io::Read;

use anyhow::{bail, Context as _, Result};
use dot::plan::Plan;
use dot::store::Store;
use serde_json::Value;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::skill::Agent;

/// The hook command lines installed by [`install`], and the
/// signature `remove` looks for: every entry whose command runs
/// a `tod hooks` command belongs to tod.
const CONTEXT_COMMAND: &str = "tod hooks context";
const REMINDER_COMMAND: &str = "tod hooks reminder";

/// The compact snapshot of the working context, ready to be
/// injected into an agent session: the current branch, its active
/// tasks with their progress, the next step of each plan. Reads
/// degrade to short notes instead of errors — a session must never
/// lose its first turn to a broken poll.
pub fn context(store: &Store) -> String {
    let Ok(branch) = store.branch() else {
        return "[tod] no git branch here (detached HEAD?) — nothing to inject\n".to_string();
    };
    let Ok(tasks) = store.load_context(&branch) else {
        return format!("[tod] branch '{branch}': no tod storage here — run 'tod init'\n");
    };
    if tasks.is_empty() {
        return format!("[tod] branch '{branch}': no active task — pick one with 'tod ls -b'\n");
    }
    let mut out = format!("[tod] branch '{branch}', {} active task(s):\n", tasks.len());
    for task in &tasks {
        let plan = store
            .locate(&task.uid)
            .ok()
            .and_then(|(_, dir)| Plan::load(&dir).ok())
            .unwrap_or_default();
        out.push_str(&format!("[tod] #{} {}", &task.uid[..5], task.description));
        if !plan.is_empty() {
            out.push_str(&format!(" [{} {}]", plan.done_count(), plan.steps.len()));
        }
        out.push('\n');
        if let Some((number, step)) = plan.steps.iter().enumerate().find(|(_, s)| !s.done) {
            out.push_str(&format!("[tod]   next: {}. {}\n", number + 1, step.text));
        }
        out.push_str(&format!("[tod]   brief: tod brief {}\n", &task.uid[..7]));
    }
    out
}

/// What the reminder command decided.
#[derive(Debug)]
pub enum Outcome {
    /// Nothing to remind: exit 0, no output.
    Quiet,
    /// Claude Code `Stop`: print on stderr, exit 2 — the agent does
    /// not stop, it receives the message and continues.
    Block(String),
    /// Vibe `post_agent`: print a structured JSON deny — the reason
    /// is injected as a follow-up message (capped at 3 per turn).
    Deny(String),
}

/// Which agent lifecycle event fired.
enum HookEvent {
    /// Claude Code, at the end of a response: `stop_hook_active` is
    /// true when Claude already continued because of a Stop hook —
    /// the loop guard: remind once, then let it stop.
    Stop { stop_hook_active: bool },
    /// Mistral Vibe, after the agent finishes a turn.
    PostAgent,
}

impl HookEvent {
    /// Parse the hook payload the agent sends on stdin.
    fn parse(input: &str) -> Option<HookEvent> {
        let value: Value = serde_json::from_str(input.trim()).ok()?;
        match value.get("hook_event_name")?.as_str()? {
            "Stop" => Some(HookEvent::Stop {
                stop_hook_active: value
                    .get("stop_hook_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }),
            "post_agent" => Some(HookEvent::PostAgent),
            _ => None,
        }
    }
}

/// The reminder decision for one hook payload. Pure: the command
/// only projects the outcome onto the process (stderr/exit code for
/// Claude Code, JSON on stdout for Vibe).
pub fn reminder(store: &Store, input: &str) -> Outcome {
    match HookEvent::parse(input) {
        Some(HookEvent::Stop {
            stop_hook_active: true,
        }) => Outcome::Quiet,
        Some(HookEvent::Stop {
            stop_hook_active: false,
        }) => pending(store).map_or(Outcome::Quiet, Outcome::Block),
        Some(HookEvent::PostAgent) => pending(store).map_or(Outcome::Quiet, Outcome::Deny),
        None => Outcome::Quiet,
    }
}

/// The unfinished plan(s) of the current branch, as a reminder
/// message — or None when there is nothing to remind.
///
/// Scoped to the **work branches**: a plan being executed lives on
/// its branch (`t/<uid>-...`, or any name given to `tod start`),
/// while main only holds planned, not-yet-started tasks — stopping
/// a session on main must never be blocked.
fn pending(store: &Store) -> Option<String> {
    let branch = store.branch().ok()?;
    if branch == "main" {
        return None;
    }
    let tasks = store.load_context(&branch).ok()?;
    let mut out = String::new();
    for task in tasks
        .iter()
        .filter(|t| t.branch.as_deref() == Some(branch.as_str()))
    {
        let Some((state, dir)) = store.locate(&task.uid).ok() else {
            continue;
        };
        if state != "todo" {
            continue;
        }
        let Ok(plan) = Plan::load(&dir) else {
            continue;
        };
        let remaining: Vec<(usize, &str)> = plan
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.done)
            .map(|(i, s)| (i + 1, s.text.as_str()))
            .collect();
        if remaining.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "[tod] unfinished plan on branch '{branch}': #{} [{} {}]\n",
            &task.uid[..5],
            plan.done_count(),
            plan.steps.len()
        ));
        for (number, text) in remaining {
            out.push_str(&format!("[tod]   {number}. {text}\n"));
        }
        out.push_str(&format!(
            "[tod] mark the completed steps (tod check {} <n>) or continue the plan.\n",
            &task.uid[..7]
        ));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Read the hook payload the agent sends on stdin (empty when the
/// process has no stdin, e.g. run by hand: the reminder stays
/// quiet, never blocking).
fn read_stdin() -> Result<String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("cannot read the hook payload on stdin")?;
    Ok(input)
}

/// The `tod hooks reminder` entry point: project the decision onto
/// the process — exit 0 and nothing for Quiet and Deny (Vibe reads
/// the JSON on stdout), exit 2 with the message on stderr for Block
/// (Claude Code does not stop and hands the message to the agent).
pub fn reminder_command() -> Result<()> {
    let input = read_stdin()?;
    if input.trim().is_empty() {
        return Ok(());
    }
    // the decision must not depend on the tod storage being healthy:
    // a broken store means nothing to remind, not a blocked session
    let store = Store::discover().ok();
    let Some(store) = store else {
        return Ok(());
    };
    match reminder(&store, &input) {
        Outcome::Quiet => Ok(()),
        Outcome::Block(message) => {
            eprint!("{message}");
            std::process::exit(2);
        }
        Outcome::Deny(message) => {
            println!(
                "{}",
                serde_json::json!({ "decision": "deny", "reason": message.trim_end() })
            );
            Ok(())
        }
    }
}

/// The `tod hooks context` entry point: the snapshot to inject.
pub fn context_command() -> Result<()> {
    let store = Store::discover().ok();
    let Some(store) = store else {
        println!("[tod] no git repository here\n");
        return Ok(());
    };
    print!("{}", context(&store));
    Ok(())
}

/// Install tod's hooks for the agent's own hook system. Cursor and
/// the generic agent have no known hook format: the skill stays
/// their only vehicle.
pub fn install(store: &Store, agent: Agent) -> Result<std::path::PathBuf> {
    match agent {
        Agent::Claude => install_claude(store),
        Agent::Vibe => install_vibe(store),
        Agent::Cursor | Agent::Generic => bail!(
            "{} has no known hook system — the tod skill stays its vehicle ('tod skill')",
            agent.label()
        ),
    }
}

/// Remove tod's hooks from the agent's config. Ok(false): none
/// were installed.
pub fn remove(store: &Store, agent: Agent) -> Result<bool> {
    match agent {
        Agent::Claude => remove_claude(store),
        Agent::Vibe => remove_vibe(store),
        Agent::Cursor | Agent::Generic => bail!(
            "{} has no known hook system — nothing to remove",
            agent.label()
        ),
    }
}

/// The hooks file of the agent, at the root of the repository.
fn hooks_file(store: &Store, agent: Agent) -> std::path::PathBuf {
    let file = match agent {
        Agent::Claude => ".claude/settings.json",
        Agent::Vibe => ".vibe/hooks.toml",
        Agent::Cursor | Agent::Generic => unreachable!("rejected by install/remove"),
    };
    store.root().join(file)
}

// ---------------------------------------------------------------------
// Claude Code — `.claude/settings.json`, JSON, merged idempotently
// ---------------------------------------------------------------------

/// Does this command line belong to tod?
fn is_tod_command(command: &str) -> bool {
    command.contains("tod hooks")
}

/// Strip every Claude Code hook entry whose command runs a tod hook.
fn strip_tod(entries: &mut Vec<Value>) {
    entries.retain(|entry| {
        !entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(is_tod_command)
                })
            })
    });
}

/// One Claude Code hook entry: a group of command hooks.
fn claude_hook(command: &str) -> Value {
    serde_json::json!({ "hooks": [ { "type": "command", "command": command } ] })
}

fn install_claude(store: &Store) -> Result<std::path::PathBuf> {
    let path = hooks_file(store, Agent::Claude);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    // an empty file is an empty object: the config may be scaffolded
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    let raw = if raw.trim().is_empty() {
        "{}".to_string()
    } else {
        raw
    };
    let mut settings: Value =
        serde_json::from_str(&raw).with_context(|| format!("cannot parse {}", path.display()))?;
    if !settings.is_object() {
        bail!("{} must be a JSON object", path.display());
    }

    if !settings.get("hooks").map(Value::is_object).unwrap_or(false) {
        settings["hooks"] = serde_json::json!({});
    }
    for (event, command) in [
        ("SessionStart", CONTEXT_COMMAND),
        ("Stop", REMINDER_COMMAND),
    ] {
        match settings["hooks"][event].as_array_mut() {
            Some(entries) => {
                strip_tod(entries);
                entries.push(claude_hook(command));
            }
            None => settings["hooks"][event] = serde_json::json!([claude_hook(command)]),
        }
    }

    std::fs::write(&path, serde_json::to_string_pretty(&settings)?)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

fn remove_claude(store: &Store) -> Result<bool> {
    let path = hooks_file(store, Agent::Claude);
    if !path.is_file() {
        return Ok(false);
    }
    let mut settings: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)
        .with_context(|| format!("cannot parse {}", path.display()))?;
    let mut removed = false;
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for entries in hooks.values_mut() {
            if let Some(array) = entries.as_array_mut() {
                let before = array.len();
                strip_tod(array);
                removed |= array.len() != before;
            }
        }
        hooks.retain(|_, entries| !entries.as_array().is_some_and(Vec::is_empty));
    }
    if settings
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|hooks| hooks.is_empty())
    {
        if let Some(object) = settings.as_object_mut() {
            object.remove("hooks");
        }
    }
    std::fs::write(&path, serde_json::to_string_pretty(&settings)?)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(removed)
}

// ---------------------------------------------------------------------
// Mistral Vibe — `.vibe/hooks.toml`, edited in place (comments preserved)
// ---------------------------------------------------------------------

/// Read the document of the vibes hooks file, or an empty one.
fn vibe_document(path: &std::path::Path) -> Result<DocumentMut> {
    if !path.is_file() {
        return Ok(DocumentMut::new());
    }
    std::fs::read_to_string(path)?
        .parse::<DocumentMut>()
        .with_context(|| format!("cannot parse {}", path.display()))
}

/// The hooks table of the document, created when missing.
fn vibe_hooks(doc: &mut DocumentMut) -> &mut ArrayOfTables {
    if doc.get("hooks").is_none() {
        doc["hooks"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    doc["hooks"]
        .as_array_of_tables_mut()
        .expect("hooks is an array of tables")
}

fn install_vibe(store: &Store) -> Result<std::path::PathBuf> {
    let path = hooks_file(store, Agent::Vibe);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let mut doc = vibe_document(&path)?;
    let hooks = vibe_hooks(&mut doc);
    hooks.retain(|table| {
        !table
            .get("command")
            .and_then(|c| c.as_str())
            .is_some_and(is_tod_command)
    });
    let mut hook = Table::new();
    hook["name"] = toml_edit::value("tod-check");
    hook["type"] = toml_edit::value("post_agent");
    hook["command"] = toml_edit::value(REMINDER_COMMAND);
    hooks.push(hook);

    std::fs::write(&path, doc.to_string())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

fn remove_vibe(store: &Store) -> Result<bool> {
    let path = hooks_file(store, Agent::Vibe);
    if !path.is_file() {
        return Ok(false);
    }
    let mut doc = vibe_document(&path)?;
    let removed = doc
        .get_mut("hooks")
        .and_then(|item| item.as_array_of_tables_mut())
        .is_some_and(|hooks| {
            let before = hooks.len();
            hooks.retain(|table| {
                !table
                    .get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(is_tod_command)
            });
            hooks.len() != before
        });
    // an empty array of tables does not linger in the file
    if doc
        .get("hooks")
        .and_then(|item| item.as_array_of_tables())
        .is_some_and(ArrayOfTables::is_empty)
    {
        doc.remove("hooks");
    }
    std::fs::write(&path, doc.to_string())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A temp directory with a real git repository and a store on it
    /// (same fixture as the commands tests).
    fn git_repo() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git init failed");
        std::fs::write(dir.path().join("README.md"), "test\n").unwrap();
        let out = Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git add failed");
        git(dir.path(), &["commit", "-m", "init"]);
        let store = Store::new(dir.path());
        store.init().unwrap();
        (dir, store)
    }

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {} failed", args.join(" "));
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn checkout(root: &std::path::Path, branch: &str) {
        let out = Command::new("git")
            .args(["checkout", "-b", branch])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git checkout failed");
    }

    fn plan_with(dir: &std::path::Path, checked: usize, total: usize) {
        let mut plan = Plan::default();
        for _ in 0..total {
            plan.add_step("a step");
        }
        for number in 1..=checked {
            plan.check_step(number).unwrap();
        }
        Plan::save(dir, &plan).unwrap();
    }

    #[test]
    fn context_without_active_task_points_at_the_backlog() {
        let (_dir, store) = git_repo();
        let text = context(&store);
        assert!(text.contains("main"), "{text}");
        assert!(text.contains("no active task"), "{text}");
    }

    #[test]
    fn context_lists_the_branch_tasks_with_their_next_step() {
        let (_dir, store) = git_repo();
        let task = store.add_in(true, "Fix the login bug", "tester").unwrap();
        store.start_task(&task.uid[..7], "main").unwrap();
        let task_dir = store.locate(&task.uid).unwrap().1;
        plan_with(&task_dir, 1, 3);

        let text = context(&store);
        assert!(text.contains(&format!("#{}", &task.uid[..5])), "{text}");
        assert!(text.contains("Fix the login bug"), "{text}");
        assert!(text.contains("[1 3]"), "{text}");
        assert!(text.contains("next: 2. a step"), "{text}");
        assert!(text.contains("tod brief"), "{text}");
    }

    #[test]
    fn reminder_stays_quiet_on_main_housekeeping_sessions() {
        let (_dir, store) = git_repo();
        // the plan lives on the branch context of main: never a nag
        let task = store.add_in(true, "planned", "tester").unwrap();
        store.start_task(&task.uid[..7], "main").unwrap();
        let task_dir = store.locate(&task.uid).unwrap().1;
        plan_with(&task_dir, 0, 2);

        assert!(matches!(
            reminder(&store, r#"{ "hook_event_name": "Stop" }"#),
            Outcome::Quiet
        ));
        assert!(matches!(
            reminder(&store, r#"{ "hook_event_name": "post_agent" }"#),
            Outcome::Quiet
        ));
    }

    #[test]
    fn reminder_blocks_a_work_branch_with_remaining_steps() {
        let (dir, store) = git_repo();
        let task = store.add_in(true, "in work", "tester").unwrap();
        let branch = format!("t/{}-in-work", &task.uid[..7]);
        checkout(dir.path(), &branch);
        store.start_task(&task.uid[..7], &branch).unwrap();
        let task_dir = store.locate(&task.uid).unwrap().1;
        plan_with(&task_dir, 1, 3);

        let outcome = reminder(&store, r#"{ "hook_event_name": "Stop" }"#);
        let Outcome::Block(message) = outcome else {
            panic!("expected a blocked stop, got {outcome:?}");
        };
        assert!(message.contains(&branch), "{message}");
        assert!(message.contains("2. a step"), "{message}");
        assert!(message.contains("3. a step"), "{message}");
        assert!(message.contains("tod check"), "{message}");

        // the same session, continued by the Stop hook: quiet, or it never stops
        let outcome = reminder(
            &store,
            r#"{ "hook_event_name": "Stop", "stop_hook_active": true }"#,
        );
        assert!(matches!(outcome, Outcome::Quiet));

        // the Vibe protocol answers a deny with the same content
        let Outcome::Deny(message) = reminder(&store, r#"{ "hook_event_name": "post_agent" }"#)
        else {
            panic!("expected a vibe deny");
        };
        assert!(message.contains("tod check"), "{message}");
    }

    #[test]
    fn reminder_stays_quiet_when_the_plan_is_fully_checked() {
        let (dir, store) = git_repo();
        let task = store.add_in(true, "checked out", "tester").unwrap();
        let branch = format!("t/{}-checked-out", &task.uid[..7]);
        checkout(dir.path(), &branch);
        store.start_task(&task.uid[..7], &branch).unwrap();
        let task_dir = store.locate(&task.uid).unwrap().1;
        plan_with(&task_dir, 3, 3);

        assert!(matches!(
            reminder(&store, r#"{ "hook_event_name": "Stop" }"#),
            Outcome::Quiet
        ));
    }

    #[test]
    fn reminder_ignores_unrelated_and_broken_payloads() {
        let (_dir, store) = git_repo();
        // unknown events, garbage, empty payloads: never a block
        for payload in [
            r#"{ "hook_event_name": "SessionStart" }"#,
            r#"{ "hook_event_name": "PreToolUse" }"#,
            "not json at all",
            "",
        ] {
            assert!(
                matches!(reminder(&store, payload), Outcome::Quiet),
                "{payload}"
            );
        }
    }

    #[test]
    fn reminder_stays_quiet_without_a_tod_storage() {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git init failed");
        let store = Store::new(dir.path());
        assert!(matches!(
            reminder(&store, r#"{ "hook_event_name": "Stop" }"#),
            Outcome::Quiet
        ));
    }

    #[test]
    fn claude_install_merges_idempotently_and_remove_is_surgical() {
        let (dir, store) = git_repo();
        let path = dir.path().join(".claude").join("settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{
  "model": "opus",
  "hooks": {
    "PreToolUse": [
      { "hooks": [ { "type": "command", "command": "make lint" } ] }
    ]
  }
}"#,
        )
        .unwrap();

        // unrelated settings and hooks survive the whole round trip
        install(&store, Agent::Claude).unwrap();
        install(&store, Agent::Claude).unwrap(); // idempotent
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(settings["model"], "opus", "{settings}");
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "make lint"
        );
        let stop: Vec<Value> = settings["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .clone()
            .into_iter()
            .filter(is_tod_entry)
            .collect();
        assert_eq!(stop.len(), 1, "{settings}"); // one, not two
        assert_eq!(
            settings["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "tod hooks context"
        );
        assert_eq!(
            settings["hooks"]["Stop"][0]["hooks"][0]["command"],
            "tod hooks reminder"
        );

        assert!(remove(&store, Agent::Claude).unwrap());
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after["model"], "opus", "{after}"); // settings kept
        assert_eq!(
            after["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "make lint",
            "{after}"
        );
        assert!(after["hooks"].get("Stop").is_none(), "{after}");
        assert!(after["hooks"].get("SessionStart").is_none(), "{after}");
        assert!(!remove(&store, Agent::Claude).unwrap()); // nothing left
    }

    /// The tod signature: a Claude Code entry whose commands run a tod hook.
    fn is_tod_entry(entry: &Value) -> bool {
        entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(is_tod_command)
                })
            })
    }

    #[test]
    fn vibe_install_preserves_comments_and_user_hooks() {
        let (dir, store) = git_repo();
        let path = dir.path().join(".vibe").join("hooks.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "# user content — must survive\n[[hooks]]\nname = \"lint\"\ntype = \"pre_tool\"\nmatch = \"bash\"\ncommand = \"make lint\"\n",
        )
        .unwrap();

        install(&store, Agent::Vibe).unwrap();
        install(&store, Agent::Vibe).unwrap(); // idempotent
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("user content"), "{content}"); // comment kept
        assert!(content.contains("name = \"lint\""), "{content}"); // user hook kept
        assert_eq!(content.matches("tod-check").count(), 1, "{content}");
        assert!(content.contains("post_agent"), "{content}");
        assert!(content.contains("tod hooks reminder"), "{content}");

        assert!(remove(&store, Agent::Vibe).unwrap());
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("user content"), "{after}");
        assert!(after.contains("name = \"lint\""), "{after}");
        assert!(!after.contains("tod-check"), "{after}");
        assert!(!remove(&store, Agent::Vibe).unwrap()); // nothing left
                                                        // and the file parses as valid TOML the whole way
        let _doc: DocumentMut = after.parse().unwrap();
    }

    #[test]
    fn cursor_and_generic_have_no_hook_system() {
        let (_dir, store) = git_repo();
        for agent in [Agent::Cursor, Agent::Generic] {
            assert!(install(&store, agent).is_err());
            assert!(remove(&store, agent).is_err());
        }
    }
}
