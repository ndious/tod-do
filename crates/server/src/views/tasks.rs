//! Task views: the JSON contract of the task endpoints, shared with
//! the web console.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct StepView {
    pub done: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub uid: String,
    pub description: String,
    pub author: String,
    pub state: String,
    /// The working branch of the task, when it has one.
    pub branch: Option<String>,
    pub tags: Vec<String>,
    pub plan: Vec<StepView>,
    pub progress: (usize, usize),
}

#[derive(Debug, Clone, Serialize)]
pub struct StateView {
    pub name: String,
    pub tasks: Vec<TaskView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub repo: String,
    pub branch: String,
    /// Every state in lifecycle order, empty ones included: the
    /// kanban columns of the console.
    pub states: Vec<StateView>,
}

/// Build the view of one task: state, branch, plan, progress.
pub fn task_view(task: &dot::task::Task, state: &str, plan: &dot::plan::Plan) -> TaskView {
    let steps: Vec<StepView> = plan
        .steps
        .iter()
        .map(|s| StepView { done: s.done, text: s.text.clone() })
        .collect();
    let done_count = steps.iter().filter(|s| s.done).count();
    TaskView {
        uid: task.uid.clone(),
        description: task.description.clone(),
        author: task.author.clone(),
        state: state.to_string(),
        branch: task.branch.clone(),
        tags: task.tags.clone(),
        progress: (done_count, steps.len()),
        plan: steps,
    }
}
