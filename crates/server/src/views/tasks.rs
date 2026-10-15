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
    pub done: bool,
    pub context: String,
    pub plan: Vec<StepView>,
    pub progress: (usize, usize),
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextView {
    pub name: String,
    pub tasks: Vec<TaskView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub repo: String,
    pub branch: String,
    pub backlog: Vec<TaskView>,
    pub contexts: Vec<ContextView>,
}

/// Build the view of one task: plan, progress, context.
pub fn task_view(task: &dot::task::Task, context: &str, project: &dot::plan::Project) -> TaskView {
    let plan: Vec<StepView> = project
        .plan(&task.uid)
        .map(|p| {
            p.steps
                .iter()
                .map(|s| StepView { done: s.done, text: s.text.clone() })
                .collect()
        })
        .unwrap_or_default();
    let done_count = plan.iter().filter(|s| s.done).count();
    TaskView {
        uid: task.uid.clone(),
        description: task.description.clone(),
        author: task.author.clone(),
        done: task.done,
        context: context.to_string(),
        progress: (done_count, plan.len()),
        plan,
    }
}
