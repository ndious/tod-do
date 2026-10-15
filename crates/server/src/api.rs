//! The JSON API of tod-server: login, repository registry, and the
//! per-repository task endpoints, all built on the dot crate.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::db::Db;
use crate::repos;
use dot::brief;
use dot::plan::Project;
use dot::store::Store;
use dot::task::Task;

/// Shared server state.
pub struct AppState {
    pub db: Db,
    pub workdir: PathBuf,
}

pub type SharedState = Arc<AppState>;

// ---------- views ----------

#[derive(Debug, Clone, serde::Serialize)]
pub struct StepView {
    pub done: bool,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskView {
    pub uid: String,
    pub description: String,
    pub author: String,
    pub done: bool,
    pub context: String,
    pub plan: Vec<StepView>,
    pub progress: (usize, usize),
}

#[derive(Debug, serde::Serialize)]
pub struct ContextView {
    pub name: String,
    pub tasks: Vec<TaskView>,
}

#[derive(Debug, serde::Serialize)]
pub struct Status {
    pub repo: String,
    pub branch: String,
    pub backlog: Vec<TaskView>,
    pub contexts: Vec<ContextView>,
}

fn task_view(task: &Task, context: &str, project: &Project) -> TaskView {
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

// ---------- helpers ----------

type ApiError = (StatusCode, String);

fn err<E: std::fmt::Display>(e: E) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad_request(msg: &str) -> ApiError {
    (StatusCode::BAD_REQUEST, msg.to_string())
}

fn unauthorized() -> ApiError {
    (StatusCode::UNAUTHORIZED, "login required".to_string())
}

/// Extract and validate the bearer token.
fn auth(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    auth_user(state, headers).map(|_| ())
}

/// Extract and validate the bearer token, returning its user.
fn auth_user(state: &AppState, headers: &HeaderMap) -> Result<String, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    state.db.session_user(token).map_err(|_| unauthorized())
}

/// Resolve the store of a registered repository (clone / pull first).
/// The final push runs in the background: a request handler must
/// not block on a slow network round trip.
fn open_store(state: &AppState, id: i64) -> Result<Store, ApiError> {
    let repo = state.db.get_repo(id).map_err(err)?;
    let root = repos::prepare(&repo, &state.workdir).map_err(err)?;
    let store = Store::new(root);
    store.set_async_push(true);
    Ok(store)
}

fn refresh_brief(store: &Store, task: &Task, context: &str) -> anyhow::Result<()> {
    if !store.md_dir().join(format!("{}.md", task.uid)).is_file() {
        return Ok(());
    }
    let project = Project::load(store)?;
    let plan = project.plan(&task.uid).cloned().unwrap_or_default();
    brief::write_brief(store, task, context, &plan)?;
    Ok(())
}

// ---------- auth ----------

#[derive(Debug, Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(
    State(state): State<SharedState>,
    Json(body): Json<LoginBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = state.db.login(&body.username, &body.password).map_err(|_| {
        (StatusCode::UNAUTHORIZED, "invalid username or password".to_string())
    })?;
    Ok(Json(serde_json::json!({ "token": token })))
}

// ---------- repos ----------

#[derive(Debug, Deserialize)]
struct AddRepoBody {
    name: String,
    url: String,
}

async fn list_repos(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::db::Repo>>, ApiError> {
    auth(&state, &headers)?;
    Ok(Json(state.db.list_repos().map_err(err)?))
}

async fn add_repo(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<AddRepoBody>,
) -> Result<Json<crate::db::Repo>, ApiError> {
    auth(&state, &headers)?;
    let repo = state.db.add_repo(&body.name, &body.url).map_err(err)?;
    // clone immediately so a bad URL is reported at registration time
    repos::prepare(&repo, &state.workdir).map_err(err)?;
    Ok(Json(repo))
}

async fn delete_repo(
    State(state): State<SharedState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    auth(&state, &headers)?;
    state.db.delete_repo(id).map_err(err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- ssh keys ----------

#[derive(Debug, serde::Serialize)]
pub struct KeyView {
    pub id: i64,
    pub public_key: String,
}

#[derive(Debug, Deserialize)]
struct AddKeyBody {
    public_key: String,
}

async fn add_key(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<AddKeyBody>,
) -> Result<Json<KeyView>, ApiError> {
    let username = auth_user(&state, &headers)?;
    // parse + canonicalize: the stored form has no comment, so the
    // SSH server can compare key texts with the wire form
    let key = russh::keys::ssh_key::PublicKey::from_openssh(body.public_key.trim())
        .map_err(|_| bad_request("invalid public key"))?;
    let normalized = crate::ssh::canonical_key(&key)
        .map_err(|_| bad_request("invalid public key"))?;
    let id = state.db.add_key(&username, &normalized).map_err(err)?;
    Ok(Json(KeyView { id, public_key: normalized }))
}

async fn list_keys(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<Vec<KeyView>>, ApiError> {
    let username = auth_user(&state, &headers)?;
    let keys = state
        .db
        .list_keys(&username)
        .map_err(err)?
        .into_iter()
        .map(|(id, public_key)| KeyView { id, public_key })
        .collect();
    Ok(Json(keys))
}

async fn delete_key(
    State(state): State<SharedState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let username = auth_user(&state, &headers)?;
    if state.db.delete_key(&username, id).map_err(err)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "no such key".to_string()))
    }
}

// ---------- tasks (scoped by repository) ----------

async fn repo_status(
    State(state): State<SharedState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Status>, ApiError> {
    auth(&state, &headers)?;
    let store = open_store(&state, id)?;
    let project = Project::load(&store).map_err(err)?;
    let branch = store.branch().map_err(err)?;
    let backlog = store.load_backlog().map_err(err)?;
    let contexts = store.contexts().map_err(err)?;

    Ok(Json(Status {
        repo: state.db.get_repo(id).map_err(err)?.name,
        branch,
        backlog: backlog.iter().map(|t| task_view(t, "backlog", &project)).collect(),
        contexts: contexts
            .iter()
            .map(|(name, tasks)| ContextView {
                name: name.clone(),
                tasks: tasks.iter().map(|t| task_view(t, name, &project)).collect(),
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize)]
struct AddTaskBody {
    description: String,
    #[serde(default)]
    context: Option<String>,
}

async fn add_task(
    State(state): State<SharedState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(body): Json<AddTaskBody>,
) -> Result<Json<TaskView>, ApiError> {
    auth(&state, &headers)?;
    if body.description.trim().is_empty() {
        return Err(bad_request("empty description"));
    }
    let store = open_store(&state, id)?;
    let task = store
        .add_in(body.context.as_deref(), body.description.trim(), "web")
        .map_err(err)?;
    let project = Project::load(&store).map_err(err)?;
    let context = body.context.unwrap_or_else(|| "backlog".to_string());
    Ok(Json(task_view(&task, &context, &project)))
}

#[derive(Debug, Deserialize)]
struct SubStepBody {
    uid: String,
    text: String,
}

async fn add_sub(
    State(state): State<SharedState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(body): Json<SubStepBody>,
) -> Result<Json<TaskView>, ApiError> {
    auth(&state, &headers)?;
    let store = open_store(&state, id)?;
    let (context, task) = store.find_task(&body.uid).map_err(err)?;
    let mut project = Project::load(&store).map_err(err)?;
    project.add_step(&task.uid, body.text.trim());
    project.save(&store).map_err(err)?;
    refresh_brief(&store, &task, &context).map_err(err)?;
    let project = Project::load(&store).map_err(err)?;
    Ok(Json(task_view(&task, &context, &project)))
}

#[derive(Debug, Deserialize)]
struct CheckStepBody {
    uid: String,
    number: usize,
}

async fn check_step(
    State(state): State<SharedState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(body): Json<CheckStepBody>,
) -> Result<Json<TaskView>, ApiError> {
    auth(&state, &headers)?;
    let store = open_store(&state, id)?;
    let (context, task) = store.find_task(&body.uid).map_err(err)?;
    let mut project = Project::load(&store).map_err(err)?;
    project.check_step(&task.uid, body.number).map_err(err)?;
    project.save(&store).map_err(err)?;
    refresh_brief(&store, &task, &context).map_err(err)?;
    let project = Project::load(&store).map_err(err)?;
    Ok(Json(task_view(&task, &context, &project)))
}

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/api/login", post(login))
        .route("/api/repos", get(list_repos).post(add_repo))
        .route("/api/repos/:id", axum::routing::delete(delete_repo))
        .route("/api/repos/:id/status", get(repo_status))
        .route("/api/repos/:id/task", post(add_task))
        .route("/api/repos/:id/sub", post(add_sub))
        .route("/api/repos/:id/check", post(check_step))
        .route("/api/keys", get(list_keys).post(add_key))
        .route("/api/keys/:id", axum::routing::delete(delete_key))
        .with_state(state)
}
