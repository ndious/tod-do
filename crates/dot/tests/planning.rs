//! Planning layer end-to-end tests: per-task plan.md and spec.md.

use std::fs;
use std::process::Command;

use dot::spec::{read, scaffold, write_scaffold, SPEC_FILE};
use dot::plan::{Plan, Step, PLAN_FILE};
use dot::store::Store;

fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("cannot create temp dir");
    let out = Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("cannot run git");
    assert!(out.status.success(), "git init failed");
    let store = Store::new(dir.path());
    store.init().expect("tod init failed");
    (dir, store)
}

#[test]
fn task_plan_round_trip() {
    let (_dir, store) = setup();
    let task = store.add("complex task", "tester", true).unwrap();
    let dir = store
        .state_dir("backlog")
        .unwrap()
        .join(&task.uid);

    let mut plan = Plan::load(&dir).unwrap();
    assert!(plan.is_empty());
    plan.add_step("investigate the bug");
    plan.add_step("write the fix");
    Plan::save(&dir, &plan).unwrap();

    // reload from the plan.md of the task directory
    let reloaded = Plan::load(&dir).unwrap();
    assert_eq!(reloaded.steps.len(), 2);
    assert!(!reloaded.steps[0].done);
    assert_eq!(reloaded.steps[0].text, "investigate the bug");
    assert_eq!(reloaded.steps[1].text, "write the fix");

    // checking a step persists
    let mut plan = Plan::load(&dir).unwrap();
    plan.check_step(1).unwrap();
    Plan::save(&dir, &plan).unwrap();
    assert_eq!(Plan::load(&dir).unwrap().done_count(), 1);
}

#[test]
fn plan_file_is_a_markdown_checklist() {
    let (_dir, store) = setup();
    let task = store.add("complex task", "tester", true).unwrap();
    let dir = store.state_dir("backlog").unwrap().join(&task.uid);

    let mut plan = Plan::default();
    plan.add_step("do things");
    plan.check_step(1).unwrap();
    Plan::save(&dir, &plan).unwrap();

    let content = fs::read_to_string(dir.join(PLAN_FILE)).unwrap();
    assert_eq!(content, "- [x] do things\n");
}

#[test]
fn spec_scaffold_lives_in_the_task_directory() {
    let (_dir, store) = setup();
    let task = store.add("complex task", "tester", false).unwrap();
    let dir = store.state_dir("todo").unwrap().join(&task.uid);

    // no spec until it is authored
    assert!(read(&dir).is_none());

    // the scaffold: a prompt skeleton, without tod metadata
    write_scaffold(&dir, &task).unwrap();
    let content = read(&dir).unwrap();
    assert!(content.starts_with("# complex task\n"));
    assert!(content.contains("## Analyse de l'existant"));
    assert!(content.contains("## Recherches"));
    assert!(content.contains("## Réflexions"));
    assert!(content.contains("## Travail à réaliser"));
    assert!(!content.contains("UID"));
    assert!(!content.contains("Branch"));
    assert!(!content.contains("Plan"));
}

#[test]
fn scaffold_helper_matches_the_file() {
    let task = store_task();
    assert!(scaffold(&task).starts_with("# Fix the login bug\n"));
}

fn store_task() -> dot::task::Task {
    dot::task::Task {
        uid: "a".repeat(40),
        description: "Fix the login bug".to_string(),
        author: "tester".to_string(),
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
        branch: Some("t/aaaaaaa-fix-the-login-bug".to_string()),
        tags: Vec::new(),
        content: None,
    }
}

#[test]
fn find_task_searches_every_state() {
    let (_dir, store) = setup();
    let ctx_task = store.add("in todo", "tester", false).unwrap();
    let backlog_task = store.add("in backlog", "tester", true).unwrap();

    let (state_ctx, found_ctx) = store.find_task(&ctx_task.uid[..5]).unwrap();
    assert_eq!(state_ctx, "todo");
    assert_eq!(found_ctx.uid, ctx_task.uid);

    let (state_backlog, found_backlog) = store.find_task(&backlog_task.uid[..5]).unwrap();
    assert_eq!(state_backlog, "backlog");
    assert_eq!(found_backlog.uid, backlog_task.uid);

    assert!(store.find_task("zzzzz").is_err());
}
