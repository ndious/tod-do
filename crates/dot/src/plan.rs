//! The planning layer: the micro-todo list of one task, in the
//! `plan.md` file of its directory, as a plain markdown checklist:
//!
//! ```markdown
//! - [x] Investigate the session expiry
//! - [ ] Add refresh token to auth.rs
//! ```

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

/// Name of the plan file, inside the task directory.
pub const PLAN_FILE: &str = "plan.md";

/// One step of a plan: a micro todo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub done: bool,
    pub text: String,
}

/// The plan of a single task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn done_count(&self) -> usize {
        self.steps.iter().filter(|s| s.done).count()
    }

    /// Load the plan of a task. A missing file is an empty plan.
    pub fn load(task_dir: &Path) -> Result<Plan> {
        let path = task_dir.join(PLAN_FILE);
        if !path.is_file() {
            return Ok(Plan::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let steps = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(parse_step)
            .collect();
        Ok(Plan { steps })
    }

    /// Save the plan of a task, removing the file when the plan
    /// becomes empty.
    pub fn save(task_dir: &Path, plan: &Plan) -> Result<()> {
        let path = task_dir.join(PLAN_FILE);
        if plan.is_empty() {
            let _ = fs::remove_file(&path);
            return Ok(());
        }
        let mut out = String::new();
        for step in &plan.steps {
            out.push_str(&stringify_step(step));
            out.push('\n');
        }
        fs::write(&path, out)
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(())
    }

    /// Add a micro todo to the plan. Returns the step number (1-based).
    pub fn add_step(&mut self, text: &str) -> usize {
        self.steps.push(Step {
            done: false,
            text: text.to_string(),
        });
        self.steps.len()
    }

    /// Toggle the step 'number' (1-based) of the plan.
    pub fn check_step(&mut self, number: usize) -> Result<Step> {
        if number == 0 || number > self.steps.len() {
            bail!(
                "no step {number} in the plan (steps: 1-{})",
                self.steps.len()
            );
        }
        let step = &mut self.steps[number - 1];
        step.done = !step.done;
        Ok(step.clone())
    }
}

fn parse_step(line: &str) -> Step {
    if let Some(text) = line.strip_prefix("- [x] ").or(line.strip_prefix("[x] ")) {
        Step { done: true, text: text.to_string() }
    } else if let Some(text) = line.strip_prefix("- [ ] ").or(line.strip_prefix("[ ] ")) {
        Step { done: false, text: text.to_string() }
    } else {
        Step { done: false, text: line.to_string() }
    }
}

fn stringify_step(step: &Step) -> String {
    let mark = if step.done { 'x' } else { ' ' };
    format!("- [{mark}] {}", step.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_parse_and_stringify_round_trip() {
        let done = parse_step("- [x] add refresh token");
        assert!(done.done);
        assert_eq!(done.text, "add refresh token");
        let todo = parse_step("- [ ] write the tests");
        assert!(!todo.done);
        assert_eq!(stringify_step(&done), "- [x] add refresh token");
        assert_eq!(stringify_step(&todo), "- [ ] write the tests");
    }

    #[test]
    fn plain_line_is_an_undone_step() {
        let step = parse_step("no checkbox here");
        assert!(!step.done);
        assert_eq!(step.text, "no checkbox here");
    }

    #[test]
    fn plan_load_save_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Plan::load(dir.path()).unwrap().is_empty());

        let mut plan = Plan::default();
        plan.add_step("first");
        plan.add_step("second");
        Plan::save(dir.path(), &plan).unwrap();
        assert_eq!(Plan::load(dir.path()).unwrap(), plan);
        assert!(dir.path().join(PLAN_FILE).is_file());

        // checking toggles, and an empty plan removes the file
        plan.check_step(1).unwrap();
        Plan::save(dir.path(), &plan).unwrap();
        assert!(Plan::load(dir.path()).unwrap().steps[0].done);
        let empty = Plan::default();
        Plan::save(dir.path(), &empty).unwrap();
        assert!(!dir.path().join(PLAN_FILE).is_file());
    }

    #[test]
    fn check_rejects_bad_numbers() {
        let mut plan = Plan::default();
        plan.add_step("only step");
        assert!(plan.check_step(0).is_err());
        assert!(plan.check_step(2).is_err());
    }
}
