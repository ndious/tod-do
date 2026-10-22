//! Spec markdown generation: the agent-facing work description of a
//! task, in the `spec.md` file of its directory.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::plan::Plan;
use crate::task::Task;
use crate::types::ISO8601_FORMAT;

/// Name of the spec file, inside the task directory.
pub const SPEC_FILE: &str = "spec.md";

/// Render the spec markdown of a task: a self-contained document
/// (objective, status, plan with progress) that can be handed to a
/// development agent.
pub fn markdown(task: &Task, state: &str, plan: &Plan) -> String {
    let short = &task.uid[..5];

    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", task.description));
    md.push_str(&format!("- **UID**: {}\n", task.uid));
    md.push_str(&format!("- **State**: {state}\n"));
    if let Some(branch) = &task.branch {
        md.push_str(&format!("- **Branch**: {branch}\n"));
    }
    if !task.tags.is_empty() {
        md.push_str(&format!("- **Tags**: {}\n", task.tags.join(", ")));
    }
    md.push_str(&format!(
        "- **Created**: {}\n",
        task.created.format(ISO8601_FORMAT)
    ));
    if !plan.is_empty() {
        md.push_str(&format!(
            "- **Progress**: {}/{}\n",
            plan.done_count(),
            plan.steps.len()
        ));
    }
    md.push_str("\n## Plan\n\n");
    if plan.is_empty() {
        md.push_str(&format!(
            "- [ ] (no step yet — `tod sub {short} \"...\"`)\n"
        ));
    } else {
        for step in &plan.steps {
            let mark = if step.done { 'x' } else { ' ' };
            md.push_str(&format!("- [{mark}] {}\n", step.text));
        }
    }
    md
}

/// Read the spec.md of a task directory, if it exists.
pub fn read(task_dir: &Path) -> Option<String> {
    fs::read_to_string(task_dir.join(SPEC_FILE)).ok()
}

/// Write the spec.md of a task, inside its directory.
pub fn write_brief(task_dir: &Path, task: &Task, state: &str, plan: &Plan) -> Result<()> {
    let mut md = markdown(task, state, plan);
    if task_dir.join(crate::scenario::SCENARIO_FILE).is_file() {
        md.push_str(
            "\n## Scénarios\n\n`scenario.feature` holds the acceptance criteria of this task, \
in French Gherkin. Convert every scenario into tests of the \
project's own framework — they are the definition of done.\n",
        );
    }
    let path = task_dir.join(SPEC_FILE);
    fs::write(&path, md)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;

    fn sample() -> Task {
        Task {
            uid: "919fea91dd3193458488c70d168c5515362cbada".to_string(),
            description: "Fix the login bug".to_string(),
            author: "tester".to_string(),
            created: Utc::now(),
            updated: Utc::now(),
            branch: Some("t/919fea9-fix-the-login-bug".to_string()),
            tags: vec!["auth".to_string()],
            content: None,
        }
    }

    #[test]
    fn markdown_is_a_self_contained_brief() {
        let mut plan = Plan::default();
        plan.add_step("patch auth.rs");
        let mut step = plan.steps[0].clone();
        step.done = true;
        plan.steps[0] = step;
        let task = sample();

        let md = markdown(&task, "todo", &plan);
        assert!(md.contains("# Fix the login bug"));
        assert!(md.contains("- **UID**: 919fea91dd3193458488c70d168c5515362cbada"));
        assert!(md.contains("- **State**: todo"));
        assert!(md.contains("- **Branch**: t/919fea9-fix-the-login-bug"));
        assert!(md.contains("- **Tags**: auth"));
        assert!(md.contains("- **Progress**: 1/1"));
    }

    #[test]
    fn an_empty_plan_is_documented() {
        let md = markdown(&sample(), "backlog", &Plan::default());
        assert!(md.contains("## Plan"));
        assert!(md.contains("no step yet"));
    }

    #[test]
    fn write_brief_goes_into_the_task_directory() {
        let dir = tempfile::tempdir().unwrap();
        write_brief(dir.path(), &sample(), "todo", &Plan::default()).unwrap();
        assert!(dir.path().join(SPEC_FILE).is_file());
    }
}
