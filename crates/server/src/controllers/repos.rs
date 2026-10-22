//! Repository controller: the registry and the per-repository task
//! endpoints, all authenticated by JWT.

use crate::{
    models::{repos, users},
    settings::Settings,
    views::repos::RepoView,
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

#[derive(Debug, Deserialize)]
pub struct TagParams {
    pub uid: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct StateParams {
    pub uid: String,
    pub state: String,
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

/// The host the client addressed, without its HTTP port: the SSH
/// clone URLs are rebuilt from it and the SSH port of the settings.
fn host_of(headers: &axum::http::HeaderMap) -> String {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    match host.rsplit_once(':') {
        // strip the http port, keeping ipv6 brackets intact
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => h.to_string(),
        _ => host.to_string(),
    }
}

/// List the managed repositories.
#[debug_handler]
async fn list(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    headers: axum::http::HeaderMap,
) -> Result<Response> {
    let settings = Settings::from_ctx(&ctx)?;
    let host = host_of(&headers);
    let list = repos::Entity::find().all(&ctx.db).await?;
    let views: Vec<RepoView> = list
        .iter()
        .map(|r| RepoView::of(r, &host, settings.ssh_port))
        .collect();
    format::json(views)
}

/// Register a repository, mirroring it immediately so a bad URL is
/// reported at registration time.
#[debug_handler]
async fn add(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    headers: axum::http::HeaderMap,
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

    let settings = Settings::from_ctx(&ctx)?;
    let host = host_of(&headers);
    format::json(RepoView::of(&repo, &host, settings.ssh_port))
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

/// Branch, states, plans and progress of a repository: the kanban
/// board of the console, one column per state.
#[debug_handler]
async fn status(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let branch = store
        .branch()
        .map_err(|e| Error::string(&format!("cannot read the branch: {e:#}")))?;

    let mut states = Vec::new();
    for (name, tasks) in store.states().map_err(wrap)? {
        let mut views = Vec::new();
        for task in &tasks {
            let dir = store.locate(&task.uid).map_err(wrap)?.1;
            let plan = dot::plan::Plan::load(&dir).map_err(wrap)?;
            views.push(task_views::task_view(task, &name, &plan, &dir));
        }
        states.push(task_views::StateView { name, tasks: views });
    }

    format::json(Status {
        repo: repo.name,
        branch,
        states,
    })
}

/// Create a task in the backlog.
#[debug_handler]
async fn add_task(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    Json(params): Json<AddTaskParams>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let task = store
        .add_in(true, params.description.trim(), "web")
        .map_err(wrap)?;
    let dir = store.locate(&task.uid).map_err(wrap)?.1;
    let plan = dot::plan::Plan::load(&dir).map_err(wrap)?;
    format::json(task_views::task_view(&task, "backlog", &plan, &dir))
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
    let (state, task) = store.find_task(&params.uid).map_err(wrap)?;
    let dir = store.locate(&task.uid).map_err(wrap)?.1;
    let mut plan = dot::plan::Plan::load(&dir).map_err(wrap)?;
    plan.add_step(params.text.trim());
    dot::plan::Plan::save(&dir, &plan).map_err(wrap)?;
    store.commit("tod: update plan").map_err(wrap)?;
    refresh_brief(&store, &task, &state)?;
    format::json(task_views::task_view(&task, &state, &plan, &dir))
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
    let (state, task) = store.find_task(&params.uid).map_err(wrap)?;
    let dir = store.locate(&task.uid).map_err(wrap)?.1;
    let mut plan = dot::plan::Plan::load(&dir).map_err(wrap)?;
    plan.check_step(params.number).map_err(wrap)?;
    dot::plan::Plan::save(&dir, &plan).map_err(wrap)?;
    store.commit("tod: update plan").map_err(wrap)?;
    refresh_brief(&store, &task, &state)?;
    format::json(task_views::task_view(&task, &state, &plan, &dir))
}

/// Move a task to another state (the kanban columns).
#[debug_handler]
async fn set_state(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    Json(params): Json<StateParams>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let task = store.set_state(&params.uid, &params.state).map_err(wrap)?;
    let (state, task) = store.find_task(&task.uid).map_err(wrap)?;
    let dir = store.locate(&task.uid).map_err(wrap)?.1;
    let plan = dot::plan::Plan::load(&dir).map_err(wrap)?;
    format::json(task_views::task_view(&task, &state, &plan, &dir))
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

/// Replace the tags of a task (an empty list clears them).
#[debug_handler]
async fn set_tags(
    _auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    Json(params): Json<TagParams>,
) -> Result<Response> {
    let repo = find_repo(&ctx, id).await?;
    let store = open_store(&ctx, &repo).await?;
    let task = store.set_tags(&params.uid, params.tags).map_err(wrap)?;
    let (state, task) = store.find_task(&task.uid).map_err(wrap)?;
    let dir = store.locate(&task.uid).map_err(wrap)?.1;
    let plan = dot::plan::Plan::load(&dir).map_err(wrap)?;
    format::json(task_views::task_view(&task, &state, &plan, &dir))
}

/// Regenerate the spec markdown, but only if it already exists.
fn refresh_brief(
    store: &dot::store::Store,
    task: &dot::task::Task,
    state: &str,
) -> Result<()> {
    let dir = store.locate(&task.uid).map_err(wrap)?.1;
    if dir.join(dot::brief::SPEC_FILE).is_file() {
        let plan = dot::plan::Plan::load(&dir).map_err(wrap)?;
        dot::brief::write_brief(&dir, task, state, &plan).map_err(wrap)?;
        store.commit(&format!("tod: brief {}", &task.uid[..7])).map_err(wrap)?;
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
        .add("/{id}/tag", post(set_tags))
        .add("/{id}/state", post(set_state))
}

/// Wrap an anyhow error (the dot crate's) as a loco error.
fn wrap(e: anyhow::Error) -> Error {
    Error::string(&format!("{e:#}"))
}
