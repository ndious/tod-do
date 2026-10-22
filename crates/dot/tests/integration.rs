//! End-to-end tests: a real (temporary) git repository per test,
//! driven through the same Store API the CLI commands use.

use std::fs;
use std::process::Command;

use dot::store::Store;
use dot::task::TASK_FILE;

/// Create a temp directory, run 'git init' inside, and a Store on it.
fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("cannot create temp dir");
    let out = Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("cannot run git");
    assert!(out.status.success(), "git init failed");
    let store = Store::new(dir.path());
    store.init().expect("dot init failed");
    (dir, store)
}

#[test]
fn add_task_to_current_branch() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();

    let task = store.add("write the tests", "tester", false).unwrap();

    let tasks = store.load_context(&branch).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].uid, task.uid);
    assert_eq!(tasks[0].description, "write the tests");
    assert_eq!(tasks[0].author, "tester");

    // one directory per task, named by the uid, holding task.tod
    let dir = store.state_dir("todo").unwrap().join(&task.uid);
    let content = fs::read_to_string(dir.join(TASK_FILE)).unwrap();
    assert!(content.contains("Description: write the tests"));
    assert!(content.contains(&format!("Branch: {branch}")));
}

#[test]
fn add_task_to_backlog() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();

    store.add("backlog item", "tester", true).unwrap();

    assert_eq!(store.load_backlog().unwrap().len(), 1);
    assert!(store.load_context(&branch).unwrap().is_empty());
}

#[test]
fn complete_task_with_partial_identifier() {
    let (_dir, store) = setup();
    let task = store.add("finish the port", "tester", true).unwrap();

    let done = store.complete(&task.uid[..5]).unwrap();
    let _ = done;
    assert_eq!(store.find_task(&task.uid).unwrap().0, "done");

    // completing twice is an error
    assert!(store.complete(&task.uid[..5]).is_err());
    // unknown identifier is an error
    assert!(store.complete("zzzzz").is_err());
}

#[test]
fn cancel_task_keeps_it_in_canceled() {
    let (_dir, store) = setup();
    let task = store.add("doomed", "tester", true).unwrap();

    store.remove(&task.uid[..5]).unwrap();
    assert!(store.load_backlog().unwrap().is_empty());
    assert_eq!(store.load_state("canceled").unwrap().len(), 1);

    // the task directory (task.tod) moved with it
    let dir = store.state_dir("canceled").unwrap().join(&task.uid);
    assert!(dir.join(TASK_FILE).is_file());
}

#[test]
fn move_task_between_backlog_and_branch() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    let task = store.add("move me", "tester", true).unwrap();
    let short = &task.uid[..5];

    // backlog -> current branch
    store.move_task(short, false).unwrap();
    assert!(store.load_backlog().unwrap().is_empty());
    assert_eq!(store.load_context(&branch).unwrap().len(), 1);

    // branch -> backlog
    store.move_task(short, true).unwrap();
    assert_eq!(store.load_backlog().unwrap().len(), 1);
    assert!(store.load_context(&branch).unwrap().is_empty());

    // the uid is preserved by the move
    assert_eq!(store.load_backlog().unwrap()[0].uid, task.uid);
}

#[test]
fn contexts_lists_only_branches_with_active_tasks() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    store.add("one task", "tester", false).unwrap();

    let contexts = store.contexts().unwrap();
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0].0, branch);
    assert_eq!(contexts[0].1.len(), 1);
}

#[test]
fn resolve_cancels_the_tasks_of_a_branch() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    store.add("doomed task", "tester", false).unwrap();

    let count = store.resolve(&branch).unwrap();
    assert_eq!(count, 1);
    assert!(store.load_context(&branch).unwrap().is_empty());
    assert_eq!(store.load_state("canceled").unwrap().len(), 1);
}

#[test]
fn export_whole_database_as_json() {
    let (dir, store) = setup();
    store.add("todo task", "tester", false).unwrap();
    store.add("backlog task", "tester", true).unwrap();

    let output = dir.path().join("export.json");
    let output_str = output.to_str().unwrap();
    let (task_count, state_count) = store.export(output_str).unwrap();
    assert_eq!(task_count, 2);
    assert_eq!(state_count, 6);

    let db: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(db["states"]["backlog"].as_array().unwrap().len(), 1);
    assert_eq!(
        db["states"]["backlog"][0]["description"].as_str().unwrap(),
        "backlog task"
    );
    assert_eq!(db["states"]["todo"].as_array().unwrap().len(), 1);
    assert_eq!(
        db["states"]["todo"][0]["description"].as_str().unwrap(),
        "todo task"
    );
}

#[test]
fn invalid_task_files_are_skipped_not_fatal() {
    let (_dir, store) = setup();
    store.add("good task", "tester", true).unwrap();

    // a corrupted task.tod must not break the listing
    let broken = store.state_dir("backlog").unwrap().join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join(TASK_FILE), "not a dot file").unwrap();

    let tasks = store.load_backlog().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].description, "good task");
}

#[test]
fn operations_require_init() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()); // no git init, no tod init
    assert!(store.add("task", "tester", false).is_err());
    assert!(store.load_backlog().is_err());
    assert!(!store.is_initialized());
}
