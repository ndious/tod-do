//! Spec markdown generation: the agent-facing brief of a task.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::plan::Plan;
use crate::store::Store;
use crate::task::Task;
use crate::types::ISO8601_FORMAT;

/// Render the spec markdown of a task: a self-contained document
/// (objective, status, plan with progress) that can be handed to a
/// development agent.
pub fn markdown(task: &Task, context: &str, plan: &Plan) -> String {
    let status = if task.done { "complete" } else { "in progress" };
    let short = &task.uid[..5];

    let mut md = String::new();
    md.push_str(&format!("# {}

", task.description));
    md.push_str(&format!("- **UID**: {}
", task.uid));
    md.push_str(&format!("- **Context**: {context}
"));
    md.push_str(&format!("- **Status**: {status}
"));
    md.push_str(&format!(
        "- **Created**: {}
",
        task.created.format(ISO8601_FORMAT)
    ));
    if !plan.is_empty() {
        md.push_str(&format!(
            "- **Progress**: {}/{}
",
            plan.done_count(),
            plan.steps.len()
        ));
    }
    md.push_str("
## Plan

");
    if plan.is_empty() {
        md.push_str(&format!(
            "- [ ] (no step yet — `tod sub {short} \"...\"`)\n"
        ));
    } else {
        for step in &plan.steps {
            let mark = if step.done { 'x' } else { ' ' };
            md.push_str(&format!("- [{mark}] {}
", step.text));
        }
    }
    md
}

/// Write the spec markdown of a task into the .dot/md folder.
pub fn write_brief(store: &Store, task: &Task, context: &str, plan: &Plan) -> Result<PathBuf> {
    let dir = store.md_dir();
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let path = dir.join(format!("{}.md", task.uid));
    fs::write(&path, markdown(task, context, plan))
        .with_context(|| format!("cannot write {}", path.display()))?;
    store.commit(&format!("tod: brief {}", &task.uid[..7]))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample() -> Task {
        Task {
            uid: "919fea91dd3193458488c70d168c5515362cbada".to_string(),
            description: "Fix the login bug".to_string(),
            author: "tester".to_string(),
            created: Utc::now(),
            updated: Utc::now(),
            done: false,
            rfr: false,
            content: None,
        }
    }

    #[test]
    fn markdown_contains_the_plan_with_progress() {
        let task = sample();
        let plan = Plan {
            steps: vec![
                Step { done: true, text: "investigate".into() },
                Step { done: false, text: "fix it".into() },
            ],
        };
        let md = markdown(&task, "feature/login", &plan);
        assert!(md.starts_with("# Fix the login bug

"));
        assert!(md.contains("- **Context**: feature/login
"));
        assert!(md.contains("- **Status**: in progress
"));
        assert!(md.contains("- **Progress**: 1/2
"));
        assert!(md.contains("- [x] investigate
"));
        assert!(md.contains("- [ ] fix it
"));
    }

    #[test]
    fn markdown_with_empty_plan_shows_a_hint() {
        let md = markdown(&sample(), "main", &Plan::default());
        assert!(md.contains("no step yet"));
        assert!(md.contains("tod sub 919fe"));
    }

    #[test]
    fn markdown_of_done_task_says_complete() {
        let mut task = sample();
        task.done = true;
        let md = markdown(&task, "main", &Plan::default());
        assert!(md.contains("- **Status**: complete
"));
    }

    use crate::plan::Step;
}
