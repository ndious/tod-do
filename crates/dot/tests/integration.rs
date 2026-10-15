//! End-to-end tests: a real (temporary) git repository per test,
//! driven through the same Store API the CLI commands use.

use std::fs;
use std::process::Command;

use dot::store::Store;

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
fn add_task_to_current_context() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();

    let task = store.add("write the tests", "tester", false).unwrap();

    let tasks = store.load_context(&branch).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].uid, task.uid);
    assert_eq!(tasks[0].description, "write the tests");
    assert_eq!(tasks[0].author, "tester");
    assert!(!tasks[0].done);

    // one extension-less file per task, named by the uid, in dot format
    let file = store.context_dir(&branch).join(&task.uid);
    assert!(file.is_file());
    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("Description: write the tests"));
    assert!(content.contains("Done: 0"));
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
    let branch = store.branch().unwrap();
    let task = store.add("finish the port", "tester", false).unwrap();

    // The dot format stores timestamps with second precision, so
    // backdate 'created' to make the 'updated' refresh observable.
    let mut stored = store.load_context(&branch).unwrap()[0].clone();
    stored.created = chrono::Utc::now() - chrono::Duration::hours(1);
    dot::store::write_task(&store.context_dir(&branch), &stored).unwrap();

    let done = store.complete(&task.uid[..5]).unwrap();
    assert!(done.done);

    let tasks = store.load_context(&branch).unwrap();
    assert!(tasks[0].done);
    assert_ne!(tasks[0].updated, tasks[0].created);

    // completing twice is an error
    assert!(store.complete(&task.uid[..5]).is_err());
    // unknown identifier is an error
    assert!(store.complete("zzzzz").is_err());
}

#[test]
fn remove_task_from_context_and_backlog() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    let ctx_task = store.add("in context", "tester", false).unwrap();
    let backlog_task = store.add("in backlog", "tester", true).unwrap();

    store.remove(&ctx_task.uid[..5], false).unwrap();
    assert!(store.load_context(&branch).unwrap().is_empty());

    store.remove(&backlog_task.uid[..5], true).unwrap();
    assert!(store.load_backlog().unwrap().is_empty());
}

#[test]
fn move_task_between_backlog_and_context() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    let task = store.add("move me", "tester", true).unwrap();
    let short = &task.uid[..5];

    // backlog -> context
    store.move_task(short, false).unwrap();
    assert!(store.load_backlog().unwrap().is_empty());
    assert_eq!(store.load_context(&branch).unwrap().len(), 1);

    // context -> backlog
    store.move_task(short, true).unwrap();
    assert_eq!(store.load_backlog().unwrap().len(), 1);
    assert!(store.load_context(&branch).unwrap().is_empty());

    // the uid is preserved by the move
    assert_eq!(store.load_backlog().unwrap()[0].uid, task.uid);
}

#[test]
fn contexts_lists_only_branches_with_todos() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    store.add("one task", "tester", false).unwrap();

    let contexts = store.contexts().unwrap();
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0].0, branch);
    assert_eq!(contexts[0].1.len(), 1);
}

#[test]
fn resolve_deletes_context_and_its_tasks() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    store.add("doomed task", "tester", false).unwrap();

    let count = store.resolve(&branch).unwrap();
    assert_eq!(count, 1);
    assert!(!store.context_dir(&branch).exists());
    assert!(store.load_context(&branch).unwrap().is_empty());

    // resolving twice fails: context no longer exists
    assert!(store.resolve(&branch).is_err());
}

#[test]
fn export_whole_database_as_json() {
    let (dir, store) = setup();
    let branch = store.branch().unwrap();
    store.add("context task", "tester", false).unwrap();
    store.add("backlog task", "tester", true).unwrap();

    let output = dir.path().join("export.json");
    let output_str = output.to_str().unwrap();
    let (backlog_count, context_count) = store.export(output_str).unwrap();
    assert_eq!(backlog_count, 1);
    assert_eq!(context_count, 1);

    let db: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(db["backlog"].as_array().unwrap().len(), 1);
    assert_eq!(
        db["backlog"][0]["description"].as_str().unwrap(),
        "backlog task"
    );
    assert!(db["contexts"][&branch].is_array());
    assert_eq!(
        db["contexts"][&branch][0]["description"].as_str().unwrap(),
        "context task"
    );
}

#[test]
fn invalid_task_files_are_skipped_not_fatal() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    store.add("good task", "tester", false).unwrap();

    // a corrupted file must not break the listing
    fs::write(store.context_dir(&branch).join("broken"), "not a dot file").unwrap();

    let tasks = store.load_context(&branch).unwrap();
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
