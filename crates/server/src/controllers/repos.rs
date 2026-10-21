//! Repository controller: the registry and the per-repository task
//! endpoints, all authenticated by JWT.

use crate::{
    models::{repos, users},
    settings::Settings,
    views::tasks::{self as task_views, Status},
};
use loco_rs::prelude::*;
use loco_rs::controller::extractor::auth::JWTWithUser;
use axum::extract::Query;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct AddRepoParams {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct AddTaskParams {
    pub description: String,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SubStepParams {
    pub uid: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct CheckStepParams {
    pub uid: String,
    pub number: usize,
}

#[derive(Debug, Deserialize)]
pub struct DiffParams {
    pub branch: String,
    pub base: Option<String>,
}

/// Open the store of a registered repository (mirror refresh first).
/// The final push runs in the background: a request handler must not
/// block on a slow network round trip.
async fn open_store(ctx: &AppContext, repo: &repos::Model) -> Result<dot::store::Store> {
    let settings = Settings::from_ctx(ctx)?;
    let root = crate::repos::prepare(repo, &PathBuf::from(settings.workdir))
        .map_err(|e| Error::string(&format!("cannot prepare repository: {e:#}")))?;
    let store = dot::store::Store::new(root);
    store.set_async_push(true);
    Ok(store)
}

/// Resolve a repository by id.
async fn find_repo(ctx: &AppContext, id: i64) -> Result<repos::Model> {
    repos::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| Error::NotFound)
}

/// List the managed repositories.
#[debug_handler]
async fn list(_auth: JWTWithUser<users::Model>, State(ctx): State<AppContext>) -> Result<Response> {
    let list = repos::Entity::find().all(&ctx.db).await?;
    format::json(list)
}

/// Register a repository, mirroring it immediately so a bad URL is
/// reported at registration time.
#[debug_handler]
async fn add(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Json(params): Json<AddRepoParams>,
) -> Result<Response> {
    let name = params.name.trim();
    if name.is_empty() || name.contains('/') {
        return Err(Error::BadRequest("invalid repository name".into()));
    }
    if repos::find_by_name(&ctx.db, name).await.is_ok() {
        return Err(Error::BadRequest("a repository with this name already exists".into()));
    }
    // An empty url creates the repository here (like GitHub's "new
    // repository"); a filled one mirrors an existing one.
    let repo = repos::ActiveModel {
        name: ActiveValue::set(name.to_string()),
        url: ActiveValue::set(params.url.trim().to_string()),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    // validate: mirror + tod branch must be usable, or roll the
    // registration back
    if let Err(e) = open_store(&ctx, &repo).await {
        let _ = repos::Entity::delete_by_id(repo.id).exec(&ctx.db).await;
        return Err(e);
    }

    format::json(repo)
}

/// Unregister a repository (the mirror stays on disk).
#[debug_handler]
async fn delete_repo(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let _ = find_repo(&ctx, id).await?;
    repos::Entity::delete_by_id(id).exec(&ctx.db).await?;
    format::empty()
}

/// Branch, backlog, contexts, plans and progress of a repository.
#[debug_handler]
async fn status(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let project = dot::plan::Project::load(&store)
        .map_err(|e| Error::string(&format!("cannot load the plan: {e:#}")))?;
    let branch = store
        .branch()
        .map_err(|e| Error::string(&format!("cannot read the branch: {e:#}")))?;
    let backlog = store.load_backlog().map_err(wrap)?;
    let contexts = store.contexts().map_err(wrap)?;

    let status = Status {
        repo: repo.name,
        branch,
        backlog: backlog.iter().map(|t| task_views::task_view(t, "backlog", &project)).collect(),
        contexts: contexts
            .iter()
            .map(|(name, tasks)| task_views::ContextView {
                name: name.clone(),
                tasks: tasks.iter().map(|t| task_views::task_view(t, name, &project)).collect(),
            })
            .collect(),
    };
    format::json(status)
}

/// Create a task in a context (or in the backlog).
#[debug_handler]
async fn add_task(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    Json(params): Json<AddTaskParams>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let task = store.add_in(params.context.as_deref(), params.description.trim(), "web").map_err(wrap)?;
    let project = dot::plan::Project::load(&store)
        .map_err(|e| Error::string(&format!("cannot load the plan: {e:#}")))?;
    format::json(task_views::task_view(&task, &params.context.unwrap_or("backlog".into()), &project))
}

/// Add a step to a task's plan.
#[debug_handler]
async fn add_sub(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    Json(params): Json<SubStepParams>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let (context, task) = store.find_task(&params.uid).map_err(wrap)?;
    let mut project = dot::plan::Project::load(&store)
        .map_err(|e| Error::string(&format!("cannot load the plan: {e:#}")))?;
    project.add_step(&task.uid, params.text.trim());
    project.save(&store).map_err(wrap)?;
    refresh_brief(&store, &task, &context)?;
    format::json(task_views::task_view(&task, &context, &project))
}

/// Toggle a step of a task's plan.
#[debug_handler]
async fn check_step(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    Json(params): Json<CheckStepParams>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let (context, task) = store.find_task(&params.uid).map_err(wrap)?;
    let mut project = dot::plan::Project::load(&store)
        .map_err(|e| Error::string(&format!("cannot load the plan: {e:#}")))?;
    project.check_step(&task.uid, params.number).map_err(wrap)?;
    project.save(&store).map_err(wrap)?;
    refresh_brief(&store, &task, &context)?;
    format::json(task_views::task_view(&task, &context, &project))
}

/// The code changes of a task's branch (its context) compared to a
/// base branch — `main` by default.
#[debug_handler]
async fn diff(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    Query(params): Query<DiffParams>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let settings = Settings::from_ctx(&ctx)?;
    let root = crate::repos::prepare(&repo, &PathBuf::from(settings.workdir))
        .map_err(|e| Error::string(&format!("cannot prepare repository: {e:#}")))?;
    let base = params.base.as_deref().unwrap_or("main");
    let d = crate::repos::diff(&root, base, params.branch.trim())
        .map_err(|e| Error::BadRequest(format!("cannot diff: {e:#}")))?;
    format::json(d)
}

/// Regenerate the spec markdown, but only if it already exists.
fn refresh_brief(
    store: &dot::store::Store,
    task: &dot::task::Task,
    context: &str,
) -> Result<()> {
    if store.md_dir().join(format!("{}.md", task.uid)).is_file() {
        let project = dot::plan::Project::load(store).map_err(wrap)?;
        let plan = project.plan(&task.uid).cloned().unwrap_or_default();
        dot::brief::write_brief(store, task, context, &plan).map_err(wrap)?;
    }
    Ok(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/repos")
        .add("/", get(list))
        .add("/", post(add))
        .add("/{id}", delete(delete_repo))
        .add("/{id}/status", get(status))
        .add("/{id}/diff", get(diff))
        .add("/{id}/task", post(add_task))
        .add("/{id}/sub", post(add_sub))
        .add("/{id}/check", post(check_step))
}

/// Wrap an anyhow error (the dot crate's) as a loco error.
fn wrap(e: anyhow::Error) -> Error {
    Error::string(&format!("{e:#}"))
}
