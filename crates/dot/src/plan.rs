//! The project planning layer: micro-todo lists for complex tasks.
//!
//! All plans live in one central document, `.dot/plan.dot`, in the
//! regular dot format: one entry per task, the key is the task uid,
//! the value is the block of steps:
//!
//! ```text
//! 919fea91dd3193458488c70d168c5515362cbada:
//!     [x] Investigate the session expiry
//!     [ ] Add refresh token to auth.rs
//! ```

use std::collections::BTreeMap;
use std::fs;

use anyhow::{bail, Context, Result};

use crate::schema::{Field, Schema};
use crate::store::Store;

/// Name of the central plan file, inside the .dot directory.
pub const PLAN_FILE: &str = "plan.dot";

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
}

/// The whole project tracking: the plan of every task, in one place.
#[derive(Debug, Clone, Default)]
pub struct Project {
    plans: BTreeMap<String, Plan>,
}

impl Project {
    /// Load the central plan document. A missing file is an empty project.
    pub fn load(store: &Store) -> Result<Self> {
        let path = store.plan_path();
        if !path.is_file() {
            return Ok(Project::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let doc = Schema::new(vec![]).parse(&content)?;

        let mut plans = BTreeMap::new();
        for (uid, field) in doc.entries() {
            if let Field::Block(lines) = field {
                let steps = lines
                    .iter()
                    .filter(|l| !l.is_empty())
                    .map(|l| parse_step(l.as_str()))
                    .collect();
                plans.insert(uid.clone(), Plan { steps });
            }
        }
        Ok(Project { plans })
    }

    /// Save the central plan document and commit it to the tod
    /// branch. Keys are task uids and are written verbatim: the
    /// schema's Title Case key rendering must not touch them, or a
    /// hex uid starting with a letter would be capitalized in the
    /// file.
    pub fn save(&self, store: &Store) -> Result<()> {
        let mut out = String::new();
        for (uid, plan) in &self.plans {
            if plan.is_empty() {
                continue;
            }
            out.push_str(uid);
            out.push_str(":\n");
            for step in &plan.steps {
                out.push('\t');
                out.push_str(&stringify_step(step));
                out.push('\n');
            }
        }
        if out.is_empty() {
            let _ = fs::remove_file(store.plan_path());
        } else {
            fs::write(store.plan_path(), &out)
                .with_context(|| format!("cannot write {}", store.plan_path().display()))?;
        }
        store.commit("tod: update plan")?;
        Ok(())
    }

    pub fn plan(&self, uid: &str) -> Option<&Plan> {
        self.plans.get(uid)
    }

    /// The plan of a task, creating an empty one if needed.
    pub fn ensure_plan(&mut self, uid: &str) -> &mut Plan {
        self.plans.entry(uid.to_string()).or_default()
    }

    /// Forget a task's plan (when the task is deleted or resolved).
    pub fn remove(&mut self, uid: &str) {
        self.plans.remove(uid);
    }

    /// Add a micro todo to a task's plan. Returns the step number (1-based).
    pub fn add_step(&mut self, uid: &str, text: &str) -> usize {
        let plan = self.ensure_plan(uid);
        plan.steps.push(Step {
            done: false,
            text: text.to_string(),
        });
        plan.steps.len()
    }

    /// Toggle the step 'number' (1-based) of a task's plan.
    pub fn check_step(&mut self, uid: &str, number: usize) -> Result<Step> {
        let plan = match self.plans.get_mut(uid) {
            Some(p) => p,
            None => bail!("no plan found for task #{uid}"),
        };
        if number == 0 || number > plan.steps.len() {
            bail!("task #{uid} has no step {number} (steps: 1-{})", plan.steps.len());
        }
        let step = &mut plan.steps[number - 1];
        step.done = !step.done;
        Ok(step.clone())
    }
}

fn parse_step(line: &str) -> Step {
    if let Some(text) = line.strip_prefix("[x] ") {
        Step { done: true, text: text.to_string() }
    } else if let Some(text) = line.strip_prefix("[ ] ") {
        Step { done: false, text: text.to_string() }
    } else {
        Step { done: false, text: line.to_string() }
    }
}

fn stringify_step(step: &Step) -> String {
    let mark = if step.done { 'x' } else { ' ' };
    format!("[{mark}] {}", step.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_parse_and_stringify_round_trip() {
        let done = parse_step("[x] add refresh token");
        assert!(done.done);
        assert_eq!(done.text, "add refresh token");
        let todo = parse_step("[ ] write the tests");
        assert!(!todo.done);
        assert_eq!(stringify_step(&done), "[x] add refresh token");
        assert_eq!(stringify_step(&todo), "[ ] write the tests");
    }

    #[test]
    fn plain_line_is_an_undone_step() {
        let step = parse_step("no checkbox here");
        assert!(!step.done);
        assert_eq!(step.text, "no checkbox here");
    }

    #[test]
    fn project_add_and_check_steps() {
        let mut project = Project::default();
        let n1 = project.add_step("uid1", "first");
        let n2 = project.add_step("uid1", "second");
        assert_eq!(n1, 1);
        assert_eq!(n2, 2);

        let step = project.check_step("uid1", 1).unwrap();
        assert!(step.done);
        assert_eq!(project.plan("uid1").unwrap().done_count(), 1);

        // toggle back
        let step = project.check_step("uid1", 1).unwrap();
        assert!(!step.done);
    }

    #[test]
    fn project_check_rejects_bad_numbers() {
        let mut project = Project::default();
        project.add_step("uid1", "only step");
        assert!(project.check_step("uid1", 0).is_err());
        assert!(project.check_step("uid1", 2).is_err());
        assert!(project.check_step("unknown", 1).is_err());
    }

    #[test]
    fn project_remove_forgets_the_plan() {
        let mut project = Project::default();
        project.add_step("uid1", "step");
        project.remove("uid1");
        assert!(project.plan("uid1").is_none());
    }
}
