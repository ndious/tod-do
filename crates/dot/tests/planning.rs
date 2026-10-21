//! Planning layer end-to-end tests: project file, plans, briefs.

use std::fs;
use std::process::Command;

use dot::brief::{markdown, write_brief};
use dot::plan::{Plan, Project, Step};
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
fn project_plan_file_round_trip() {
    let (_dir, store) = setup();
    let task = store.add("complex task", "tester", false).unwrap();

    let mut project = Project::load(&store).unwrap();
    project.add_step(&task.uid, "investigate the bug");
    project.add_step(&task.uid, "write the fix");
    project.save(&store).unwrap();

    // reload from the central plan.dot file
    let reloaded = Project::load(&store).unwrap();
    let plan = reloaded.plan(&task.uid).unwrap();
    assert_eq!(plan.steps.len(), 2);
    assert!(!plan.steps[0].done);
    assert_eq!(plan.steps[0].text, "investigate the bug");
    assert_eq!(plan.steps[1].text, "write the fix");
}

#[test]
fn project_plan_file_keeps_done_state() {
    let (_dir, store) = setup();
    let task = store.add("complex task", "tester", false).unwrap();

    let mut project = Project::load(&store).unwrap();
    project.add_step(&task.uid, "step one");
    project.check_step(&task.uid, 1).unwrap();
    project.save(&store).unwrap();

    let reloaded = Project::load(&store).unwrap();
    let plan = reloaded.plan(&task.uid).unwrap();
    assert_eq!(plan.done_count(), 1);
    assert!(plan.steps[0].done);
}

#[test]
fn project_file_is_valid_dot_format() {
    let (_dir, store) = setup();
    let task = store.add("complex task", "tester", false).unwrap();

    let mut project = Project::load(&store).unwrap();
    project.add_step(&task.uid, "do things");
    project.check_step(&task.uid, 1).unwrap();
    project.save(&store).unwrap();

    let content = fs::read_to_string(store.plan_path()).unwrap();
    assert!(content.contains(&format!("{}:", task.uid)));
    assert!(content.contains("
	[x] do things
"));
}

#[test]
fn project_load_without_file_is_empty() {
    let (_dir, store) = setup();
    let project = Project::load(&store).unwrap();
    assert!(project.plan("whatever").is_none());
}

#[test]
fn brief_markdown_written_to_md_folder() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    let task = store.add("complex task", "tester", false).unwrap();

    let mut project = Project::load(&store).unwrap();
    project.add_step(&task.uid, "first");
    project.add_step(&task.uid, "second");
    project.check_step(&task.uid, 1).unwrap();
    let plan = project.plan(&task.uid).unwrap().clone();

    let path = write_brief(&store, &task, &branch, &plan).unwrap();
    assert_eq!(path, store.md_dir().join(format!("{}.md", task.uid)));
    assert!(path.is_file());

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.starts_with("# complex task
"));
    assert!(content.contains(&format!("- **Context**: {branch}
")));
    assert!(content.contains("- **Progress**: 1/2
"));
    assert!(content.contains("- [x] first
"));
    assert!(content.contains("- [ ] second
"));
}

#[test]
fn markdown_is_agent_friendly() {
    // The brief must be self-contained: objective, plan, done state.
    let task = store_task();
    let plan = Plan {
        steps: vec![
            Step { done: true, text: "read the code".into() },
            Step { done: false, text: "patch auth.rs".into() },
        ],
    };
    let md = markdown(&task, "feature/login", &plan);
    assert!(md.contains("# Fix the login bug"));
    assert!(md.contains("## Plan"));
    assert!(md.contains("- [ ] patch auth.rs"));
    assert!(md.contains("- **Progress**: 1/2"));
}

fn store_task() -> dot::task::Task {
    dot::task::Task {
        uid: "a".repeat(40),
        description: "Fix the login bug".to_string(),
        author: "tester".to_string(),
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
        done: false,
            rfr: false,
        content: None,
    }
}

#[test]
fn find_task_searches_backlog_then_contexts() {
    let (_dir, store) = setup();
    let branch = store.branch().unwrap();
    let ctx_task = store.add("in context", "tester", false).unwrap();
    let backlog_task = store.add("in backlog", "tester", true).unwrap();

    let (loc_ctx, found_ctx) = store.find_task(&ctx_task.uid[..5]).unwrap();
    assert_eq!(loc_ctx, branch);
    assert_eq!(found_ctx.uid, ctx_task.uid);

    let (loc_backlog, found_backlog) = store.find_task(&backlog_task.uid[..5]).unwrap();
    assert_eq!(loc_backlog, "backlog");
    assert_eq!(found_backlog.uid, backlog_task.uid);

    assert!(store.find_task("zzzzz").is_err());
}
