//! Gherkin scenarios: the acceptance criteria of a task, in the
//! `scenario.feature` file of its directory.
//!
//! tod never runs them: the scenarios are versioned with the task
//! and converted into tests of the target project's framework by
//! the development agent (the spec.md says so). Gherkin is the
//! format of choice: six keywords, French support, and the best
//! understood test format by the models.

use std::fs;
use std::path::Path;

use crate::task::Task;

/// Name of the scenario file, inside the task directory.
pub const SCENARIO_FILE: &str = "scenario.feature";

/// The scaffold of a task's scenario file, in French Gherkin:
/// the starting point the user (or their AI assistant) fills in.
pub fn scaffold(task: &Task) -> String {
    format!(
        "# language: fr\nFonctionnalité: {}\n\n  Scénario: premier scénario\n    Étant donné ...\n    Quand ...\n    Alors ...\n",
        task.description
    )
}

/// Read the scenario file of a task directory, if it exists.
pub fn read(task_dir: &Path) -> Option<String> {
    fs::read_to_string(task_dir.join(SCENARIO_FILE)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn scaffold_is_valid_french_gherkin() {
        let task = Task {
            uid: "a".repeat(40),
            description: "Fix the login bug".to_string(),
            author: "tester".to_string(),
            created: Utc::now(),
            updated: Utc::now(),
            branch: None,
            tags: Vec::new(),
            content: None,
        };
        let s = scaffold(&task);
        assert!(s.starts_with("# language: fr\n"));
        assert!(s.contains("Fonctionnalité: Fix the login bug"));
        assert!(s.contains("Étant donné"));
        assert!(s.contains("Quand"));
        assert!(s.contains("Alors"));
    }

    #[test]
    fn read_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_none());
        fs::write(dir.path().join(SCENARIO_FILE), "Fonctionnalité: x").unwrap();
        assert!(read(dir.path()).is_some());
    }
}
