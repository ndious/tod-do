//! The spec of a task: its **detailed prompt**, in the `spec.md`
//! file of its directory.
//!
//! The spec is never generated: it is written by the human and the
//! development agent — the analysis of the existing code, the
//! research, the reflections, and the work to realize. It carries
//! none of tod's metadata (uid, state, branch) and not the plan:
//! those live in `task.tod` and `plan.md`. `spec.rs` only provides
//! the scaffold the prompt starts from.

use std::fs;
use std::path::Path;

use crate::task::Task;

/// Name of the spec file, inside the task directory.
pub const SPEC_FILE: &str = "spec.md";

/// The scaffold of a task's spec: the prompt skeleton the human or
/// the agent fills in — analysis of the existing code first,
/// research and reflections, then the work to realize.
pub fn scaffold(task: &Task) -> String {
    format!(
        "# {}\n\n\
         ## Analyse de l'existant\n\n\n\n\
         ## Recherches\n\n\n\n\
         ## Réflexions\n\n\n\n\
         ## Travail à réaliser\n\n\n",
        task.description
    )
}

/// Read the spec of a task directory, if it exists.
pub fn read(task_dir: &Path) -> Option<String> {
    fs::read_to_string(task_dir.join(SPEC_FILE)).ok()
}

/// Write the spec of a task, inside its directory. Only used by
/// the scaffold path: an authored spec is edited in place, never
/// overwritten.
pub fn write_scaffold(task_dir: &Path, task: &Task) -> std::io::Result<()> {
    fs::write(task_dir.join(SPEC_FILE), scaffold(task))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample() -> Task {
        Task {
            uid: "a".repeat(40),
            description: "Fix the login bug".to_string(),
            author: "tester".to_string(),
            created: Utc::now(),
            updated: Utc::now(),
            branch: None,
            tags: Vec::new(),
            content: None,
        }
    }

    #[test]
    fn scaffold_is_a_prompt_skeleton() {
        let s = scaffold(&sample());
        assert!(s.starts_with("# Fix the login bug\n"));
        // the prompt sections, in order
        assert!(s.contains("## Analyse de l'existant"));
        assert!(s.contains("## Recherches"));
        assert!(s.contains("## Réflexions"));
        assert!(s.contains("## Travail à réaliser"));
        // no tod metadata, no plan
        assert!(!s.contains("UID"));
        assert!(!s.contains("Branch"));
        assert!(!s.contains("Progress"));
        assert!(!s.contains("Plan"));
    }

    #[test]
    fn read_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_none());
        write_scaffold(dir.path(), &sample()).unwrap();
        assert!(read(dir.path()).is_some());
    }
}
