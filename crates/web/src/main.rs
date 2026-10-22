//! tod web console — Yew (CSR) app: login, repository registry,
//! and the task board of each repository. Served by the tod server.

use gloo_net::http::Request;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

/// The current value of a text input event.
fn input_value(e: InputEvent) -> String {
    e.target()
        .expect("input without target")
        .dyn_into::<web_sys::HtmlInputElement>()
        .expect("not an input")
        .value()
}

/// The current value of a select event.
fn select_value(e: InputEvent) -> String {
    e.target()
        .expect("select without target")
        .dyn_into::<web_sys::HtmlSelectElement>()
        .expect("not a select")
        .value()
}

// ---------- api types ----------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepView {
    pub done: bool,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskView {
    pub uid: String,
    pub description: String,
    pub author: String,
    pub state: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub plan: Vec<StepView>,
    pub progress: (usize, usize),
    #[serde(default)]
    pub scenario: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateView {
    pub name: String,
    pub tasks: Vec<TaskView>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub states: Vec<StateView>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RepoView {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub clone_url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountView {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KeyView {
    pub id: i64,
    #[serde(default)]
    pub public_key: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiffResp {
    #[serde(default)]
    pub stat: String,
    #[serde(default)]
    pub diff: String,
    #[serde(default)]
    pub truncated: bool,
}

/// Percent-encode a value for a query string (branch names are slugs,
/// but they can carry accents).
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------- api helpers ----------

// ---------- token persistence ----------

/// The bearer token survives page refreshes: it lives in the browser
/// local storage.
const TOKEN_KEY: &str = "tod-token";

fn save_token(token: &str) {
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
    {
        let _ = storage.set_item(TOKEN_KEY, token);
    }
}

fn stored_token() -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(TOKEN_KEY).ok().flatten())
        .filter(|t| !t.is_empty())
}

fn clear_token() {
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
    {
        let _ = storage.remove_item(TOKEN_KEY);
    }
}

// ---------- routing ----------

/// The pages of the console, derived from the URL hash:
/// `#/` (repositories), `#/new` (creation), `#/repo/3` (a board).
#[derive(Clone, PartialEq)]
enum Route {
    Repos,
    NewRepo,
    Account,
    Repo(i64),
}

fn current_route() -> Route {
    let hash = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .unwrap_or_default();
    let path = hash.trim().trim_start_matches('#').trim_matches('/');
    if path == "new" {
        return Route::NewRepo;
    }
    if path == "account" {
        return Route::Account;
    }
    if let Some(id) = path.strip_prefix("repo/") {
        if let Ok(id) = id.parse() {
            return Route::Repo(id);
        }
    }
    Route::Repos
}

/// Navigate to a hash route: the hashchange event updates the state.
fn navigate(hash: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.location().set_hash(hash);
    }
}

// ---------- api helpers ----------

/// An api call either succeeds, fails, or is rejected because the
/// token is no longer valid.
enum ApiOutcome<T> {
    Ok(T),
    Unauthorized,
    Failed,
}

async fn api_get<T: for<'de> Deserialize<'de>>(token: &str, path: &str) -> ApiOutcome<T> {
    let resp = match Request::get(path)
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(_) => return ApiOutcome::Failed,
    };
    if resp.status() == 401 {
        return ApiOutcome::Unauthorized;
    }
    match resp.json::<T>().await {
        Ok(value) => ApiOutcome::Ok(value),
        Err(_) => ApiOutcome::Failed,
    }
}

#[derive(Deserialize)]
struct LoginResp { token: String }

async fn api_login(u: String, p: String) -> Option<String> {
    let body = serde_json::json!({ "username": u, "password": p });
    let request = Request::post("/api/login").json(&body).ok()?;
    let resp = request.send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    resp.json::<LoginResp>().await.ok().map(|r| r.token)
}

/// POST returning the parsed response (the caller decides what to
/// do with an unauthorized or a failure).
async fn api_post_json<T: for<'de> Deserialize<'de>>(
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> ApiOutcome<T> {
    let request = match Request::post(path)
        .header("Authorization", &format!("Bearer {token}"))
        .json(body)
    {
        Ok(request) => request,
        Err(_) => return ApiOutcome::Failed,
    };
    let resp = match request.send().await {
        Ok(resp) => resp,
        Err(_) => return ApiOutcome::Failed,
    };
    if resp.status() == 401 {
        return ApiOutcome::Unauthorized;
    }
    if !resp.ok() {
        return ApiOutcome::Failed;
    }
    match resp.json::<T>().await {
        Ok(value) => ApiOutcome::Ok(value),
        Err(_) => ApiOutcome::Failed,
    }
}

async fn api_post(token: &str, path: &str, body: &serde_json::Value) -> bool {
    let request = match Request::post(path)
        .header("Authorization", &format!("Bearer {token}"))
        .json(body)
    {
        Ok(request) => request,
        Err(_) => return false,
    };
    match request.send().await {
        Ok(resp) => resp.ok(),
        Err(_) => false,
    }
}

/// DELETE returning whether it succeeded.
async fn api_delete(token: &str, path: &str) -> bool {
    match Request::delete(path)
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
    {
        Ok(resp) => resp.ok(),
        Err(_) => false,
    }
}

// ---------- components ----------

#[derive(Properties, PartialEq)]
pub struct TaskCardProps {
    pub task: TaskView,
    pub on_open: Callback<TaskView>,
}

#[function_component(TaskCard)]
pub fn task_card(props: &TaskCardProps) -> Html {
    let task = &props.task;

    let onclick = {
        let on_open = props.on_open.clone();
        let task = props.task.clone();
        Callback::from(move |_| on_open.emit(task.clone()))
    };

    let class = if task.state == "done" || task.state == "canceled" {
        "task done card bg-base-100"
    } else {
        "task card bg-base-100"
    };
    let short = &task.uid[..task.uid.len().min(7)];

    html! {
        <li class={class} {onclick}>
            <span class="desc">{ task.description.clone() }</span>
            <span class="uid badge badge-ghost badge-sm font-mono">{ short }</span>
            if let Some(branch) = &task.branch {
                <span class="badge badge-ghost badge-sm font-mono">{ branch.clone() }</span>
            }
            { for task.tags.iter().map(|tag| html! {
                <span class="badge badge-outline badge-sm">{ tag.clone() }</span>
            }) }
            if !task.plan.is_empty() {
                <span class="badge badge-primary badge-sm">{ format!("{}/{}", task.progress.0, task.progress.1) }</span>
            }
        </li>
    }
}

/// Look a task up in the project status by its identifier.
fn find_task(status: &Status, uid: &str) -> Option<TaskView> {
    status
        .states
        .iter()
        .flat_map(|s| s.tasks.iter())
        .find(|t| t.uid == uid)
        .cloned()
}

/// Copy text to the system clipboard (nothing happens when the
/// browser refuses, e.g. insecure context).
fn copy_to_clipboard(text: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().clipboard().write_text(text);
    }
}

#[derive(Properties, PartialEq)]
pub struct CloneBarProps {
    pub clone_url: String,
}

/// The clone address of a repository, ready to copy-paste.
#[function_component(CloneBar)]
pub fn clone_bar(props: &CloneBarProps) -> Html {
    let copied = use_state(|| false);

    let onclick = {
        let url = props.clone_url.clone();
        let copied = copied.clone();
        Callback::from(move |_| {
            copy_to_clipboard(&url);
            copied.set(true);
        })
    };

    html! {
        <div class="clone">
            <input
                type="text"
                class="input input-bordered input-sm font-mono"
                readonly={true}
                value={props.clone_url.clone()}
            />
            <button class="btn btn-sm" {onclick}>
                { if *copied { "copied" } else { "copy" } }
            </button>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct AccountPageProps {
    pub token: String,
}

/// The account page: the logged-in user, their password, and their
/// SSH keys — the ones the git clients authenticate with.
#[function_component(AccountPage)]
pub fn account_page(props: &AccountPageProps) -> Html {
    let account = use_state(AccountView::default);
    let keys = use_state(Vec::<KeyView>::new);
    let message = use_state(String::new);
    let current_pw = use_state(String::new);
    let new_pw = use_state(String::new);
    let new_key = use_state(String::new);

    let refresh = {
        let token = props.token.clone();
        let account = account.clone();
        let keys = keys.clone();
        Callback::from(move |_| {
            let (token, account, keys) = (token.clone(), account.clone(), keys.clone());
            spawn_local(async move {
                match api_get::<AccountView>(&token, "/api/account").await {
                    ApiOutcome::Ok(a) => account.set(a),
                    ApiOutcome::Unauthorized => {
                        clear_token();
                        navigate("#/");
                    }
                    ApiOutcome::Failed => {}
                }
                if let ApiOutcome::Ok(k) = api_get::<Vec<KeyView>>(&token, "/api/keys").await {
                    keys.set(k);
                }
            });
        })
    };
    {
        let refresh = refresh.clone();
        use_effect_with((), move |_| {
            refresh.emit(());
            || {}
        });
    }

    let on_submit_password = {
        let token = props.token.clone();
        let current_pw = current_pw.clone();
        let new_pw = new_pw.clone();
        let message = message.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let (current, new) = ((*current_pw).clone(), (*new_pw).clone());
            if current.is_empty() || new.len() < 8 {
                message.set("the new password needs at least 8 characters".to_string());
                return;
            }
            let (token, message) = (token.clone(), message.clone());
            let (current_pw, new_pw) = (current_pw.clone(), new_pw.clone());
            spawn_local(async move {
                let body = serde_json::json!({ "current": current, "new": new });
                if api_post(&token, "/api/account/password", &body).await {
                    message.set("password changed".to_string());
                    current_pw.set(String::new());
                    new_pw.set(String::new());
                } else {
                    message.set("cannot change the password (wrong current password?)".to_string());
                }
            });
        })
    };

    let on_add_key = {
        let token = props.token.clone();
        let new_key = new_key.clone();
        let message = message.clone();
        let refresh = refresh.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let key = (*new_key).clone();
            if key.trim().is_empty() {
                return;
            }
            let (token, message, refresh) = (token.clone(), message.clone(), refresh.clone());
            let new_key = new_key.clone();
            spawn_local(async move {
                let body = serde_json::json!({ "public_key": key });
                if api_post(&token, "/api/keys", &body).await {
                    message.set(String::new());
                    new_key.set(String::new());
                    refresh.emit(());
                } else {
                    message.set("cannot add the key (invalid, or already registered?)".to_string());
                }
            });
        })
    };

    let key_rows = (*keys)
        .iter()
        .map(|k| {
            let token = props.token.clone();
            let id = k.id;
            let refresh = refresh.clone();
            let onclick = Callback::from(move |_| {
                let (token, id, refresh) = (token.clone(), id, refresh.clone());
                spawn_local(async move {
                    if api_delete(&token, &format!("/api/keys/{id}")).await {
                        refresh.emit(());
                    }
                });
            });
            // "ssh-ed25519 AAAA… user@laptop" -> type + short blob
            let display = match k.public_key.split_once(' ') {
                Some((kind, blob)) => format!("{kind} {}…", &blob[..blob.len().min(24)]),
                None => k.public_key.clone(),
            };
            html! {
                <li class="key">
                    <code>{ display }</code>
                    <button class="btn btn-ghost btn-xs" {onclick}>{ "delete" }</button>
                </li>
            }
        })
        .collect::<Html>();

    html! {
        <section class="account">
            <h2>{ format!("account — {} ({})", (*account).email, (*account).name) }</h2>

            <div class="card bg-base-100 account-card">
                <h3>{ "change password" }</h3>
                <form class="account-form" onsubmit={on_submit_password}>
                    <input
                        type="password"
                        class="input input-bordered"
                        placeholder="current password"
                        value={(*current_pw).clone()}
                        oninput={Callback::from(move |e: InputEvent| current_pw.set(input_value(e)))}
                    />
                    <input
                        type="password"
                        class="input input-bordered"
                        placeholder="new password (8+ characters)"
                        value={(*new_pw).clone()}
                        oninput={Callback::from(move |e: InputEvent| new_pw.set(input_value(e)))}
                    />
                    <button type="submit" class="btn btn-primary">{ "change" }</button>
                </form>
            </div>

            <div class="card bg-base-100 account-card">
                <h3>{ "ssh keys" }</h3>
                <p class="meta">
                    { "the keys authenticate your git pushes: ssh://git@host:2222/<repo>.git" }
                </p>
                <ul class="keys">{ key_rows }</ul>
                <form class="account-form" onsubmit={on_add_key}>
                    <input
                        type="text"
                        class="input input-bordered"
                        placeholder="ssh-ed25519 AAAA… you@laptop"
                        value={(*new_key).clone()}
                        oninput={Callback::from(move |e: InputEvent| new_key.set(input_value(e)))}
                    />
                    <button type="submit" class="btn btn-primary">{ "add key" }</button>
                </form>
            </div>

            if !(*message).is_empty() {
                <p class="meta">{ (*message).clone() }</p>
            }
        </section>
    }
}

#[derive(Properties, PartialEq)]
pub struct TaskDetailProps {
    pub token: String,
    pub repo_id: i64,
    pub task: TaskView,
    pub on_close: Callback<()>,
    pub on_refresh: Callback<()>,
}

/// The overlay opened by a click on a task card: the full task
/// detail, with its plan (steps can be checked, new ones added).
#[function_component(TaskDetail)]
pub fn task_detail(props: &TaskDetailProps) -> Html {
    let task = &props.task;
    let new_step = use_state(String::new);

    // the diff of the task's branch, loaded on demand
    #[derive(Clone)]
    enum DiffView {
        Hidden,
        Loading,
        Shown(DiffResp),
        Failed(String),
    }
    let diff_view = use_state(|| DiffView::Hidden);

    let on_toggle_diff = {
        let token = props.token.clone();
        let repo_id = props.repo_id;
        let branch = task.branch.clone();
        let diff_view = diff_view.clone();
        Callback::from(move |_| {
            let Some(branch) = branch.clone() else {
                return;
            };
            // shown? a second click hides it again
            if let DiffView::Shown(_) = &*diff_view {
                diff_view.set(DiffView::Hidden);
                return;
            }
            diff_view.set(DiffView::Loading);
            let (token, branch, diff_view) = (token.clone(), branch.clone(), diff_view.clone());
            spawn_local(async move {
                let path = format!("/api/repos/{repo_id}/diff?branch={}", urlencode(&branch));
                match api_get::<DiffResp>(&token, &path).await {
                    ApiOutcome::Ok(d) => diff_view.set(DiffView::Shown(d)),
                    ApiOutcome::Unauthorized | ApiOutcome::Failed => {
                        diff_view.set(DiffView::Failed("cannot load the diff".to_string()))
                    }
                }
            });
        })
    };

    // Escape closes the overlay
    {
        let on_close = props.on_close.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window().expect("no window");
            let on_close = on_close.clone();
            let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                if e.key() == "Escape" {
                    on_close.emit(());
                }
            });
            window
                .add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref())
                .expect("cannot listen to keydown");
            move || {
                let _ = window.remove_event_listener_with_callback(
                    "keydown",
                    on_key.as_ref().unchecked_ref(),
                );
            }
        });
    }

    let on_check = {
        let token = props.token.clone();
        let path = format!("/api/repos/{}/check", props.repo_id);
        let uid = task.uid.clone();
        let on_refresh = props.on_refresh.clone();
        Callback::from(move |n: usize| {
            let (token, path, uid) = (token.clone(), path.clone(), uid.clone());
            let on_refresh = on_refresh.clone();
            spawn_local(async move {
                let body = serde_json::json!({ "uid": uid, "number": n });
                api_post(&token, &path, &body).await;
                on_refresh.emit(());
            });
        })
    };

    let steps = task
        .plan
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let n = i + 1;
            let on_check = on_check.clone();
            let onclick = Callback::from(move |_| on_check.emit(n));
            let class = if s.done { "step done" } else { "step" };
            html! {
                <li class={class} {onclick}>
                    <input type="checkbox" class="checkbox checkbox-xs" checked={s.done} />
                    <span>{ s.text.clone() }</span>
                </li>
            }
        })
        .collect::<Html>();

    let on_submit_step = {
        let token = props.token.clone();
        let path = format!("/api/repos/{}/sub", props.repo_id);
        let uid = task.uid.clone();
        let new_step = new_step.clone();
        let on_refresh = props.on_refresh.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let text = (*new_step).clone();
            if text.trim().is_empty() {
                return;
            }
            let (token, path, uid) = (token.clone(), path.clone(), uid.clone());
            let on_refresh = on_refresh.clone();
            spawn_local(async move {
                let body = serde_json::json!({ "uid": uid, "text": text });
                api_post(&token, &path, &body).await;
                on_refresh.emit(());
            });
        })
    };

    let on_close = {
        let on_close = props.on_close.clone();
        Callback::from(move |_| on_close.emit(()))
    };

    // the tag editor: the input holds the current tags, the save
    // button replaces them
    let tag_input = use_state(|| task.tags.join(", "));
    let on_save_tags = {
        let token = props.token.clone();
        let path = format!("/api/repos/{}/tag", props.repo_id);
        let uid = task.uid.clone();
        let tag_input = tag_input.clone();
        let on_refresh = props.on_refresh.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let tags: Vec<String> = (*tag_input)
                .split(',')
                .map(str::to_string)
                .collect();
            let (token, path, uid) = (token.clone(), path.clone(), uid.clone());
            let on_refresh = on_refresh.clone();
            spawn_local(async move {
                let body = serde_json::json!({ "uid": uid, "tags": tags });
                api_post(&token, &path, &body).await;
                on_refresh.emit(());
            });
        })
    };

    let short = &task.uid[..task.uid.len().min(7)];

    html! {
        <div class="modal modal-open" onclick={on_close.clone()}>
            <div class="modal-box" onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}>
                <h3 class="text-lg font-bold">{ task.description.clone() }</h3>
                <p class="meta">
                    { format!("{} — by {}", short, task.author) }
                    { for task.tags.iter().map(|tag| html! {
                        <span class="badge badge-outline badge-sm">{ tag.clone() }</span>
                    }) }
                </p>
                <div class="state-bar">
                    <select
                        class="select select-bordered select-sm"
                        oninput={
                            let token = props.token.clone();
                            let path = format!("/api/repos/{}/state", props.repo_id);
                            let uid = task.uid.clone();
                            let on_refresh = props.on_refresh.clone();
                            Callback::from(move |e: InputEvent| {
                                let state = select_value(e);
                                let (token, path, uid, state) = (
                                    token.clone(),
                                    path.clone(),
                                    uid.clone(),
                                    state,
                                );
                                let on_refresh = on_refresh.clone();
                                spawn_local(async move {
                                    let body = serde_json::json!({ "uid": uid, "state": state });
                                    api_post(&token, &path, &body).await;
                                    on_refresh.emit(());
                                });
                            })
                        }
                    >
                        { for ["backlog", "todo", "review", "test", "done", "canceled"].iter().map(|s| html! {
                            <option value={*s} selected={*s == task.state}>{ *s }</option>
                        }) }
                    </select>
                    if let Some(branch) = &task.branch {
                        <span class="badge badge-ghost badge-sm font-mono">{ branch.clone() }</span>
                    }
                </div>
                <form class="tags-form" onsubmit={on_save_tags}>
                    <input
                        type="text"
                        class="input input-bordered input-sm"
                        placeholder="tags, comma separated"
                        value={(*tag_input).clone()}
                        oninput={Callback::from(move |e: InputEvent| tag_input.set(input_value(e)))}
                    />
                    <button type="submit" class="btn btn-sm">{ "save tags" }</button>
                </form>
                <ol class="plan">{ steps }</ol>
                if let Some(scenario) = &task.scenario {
                    <h4>{ "scénarios" }</h4>
                    <pre class="gherkin">{ scenario.clone() }</pre>
                }
                <form class="sub" onsubmit={on_submit_step}>
                    <input
                        type="text"
                        class="input input-bordered input-sm"
                        placeholder="new micro todo…"
                        value={(*new_step).clone()}
                        oninput={Callback::from(move |e: InputEvent| new_step.set(input_value(e)))}
                    />
                    <button type="submit" class="btn btn-sm">{ "+" }</button>
                </form>
                if task.branch.is_some() {
                    <button class="btn btn-ghost btn-xs" onclick={on_toggle_diff.clone()}>
                        { match (*diff_view).clone() {
                            DiffView::Hidden => "show diff".to_string(),
                            DiffView::Loading => "loading…".to_string(),
                            DiffView::Shown(_) => "hide diff".to_string(),
                            DiffView::Failed(_) => "show diff".to_string(),
                        } }
                    </button>
                }
                { match (*diff_view).clone() {
                    DiffView::Hidden => html! {},
                    DiffView::Loading => html! { <p class="meta">{ "loading diff…" }</p> },
                    DiffView::Failed(e) => html! { <p class="error">{ e }</p> },
                    DiffView::Shown(d) => {
                        let lines = d
                            .diff
                            .lines()
                            .map(|l| {
                                let class = if l.starts_with('+') {
                                    "add"
                                } else if l.starts_with('-') {
                                    "del"
                                } else if l.starts_with('@') {
                                    "hunk"
                                } else {
                                    "ctx"
                                };
                                html! { <span class={class}>{ format!("{l}\n") }</span> }
                            })
                            .collect::<Html>();
                        html! {
                            <>
                                <pre class="stat">{ d.stat }</pre>
                                if d.truncated {
                                    <p class="meta">{ "diff truncated at 256 KiB" }</p>
                                }
                                <pre class="diff">{ lines }</pre>
                            </>
                        }
                    }
                } }
                <div class="modal-action">
                    <button class="btn btn-sm" onclick={on_close}>{ "close" }</button>
                </div>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct AddFormProps {
    pub token: String,
    pub repo_id: i64,
    pub on_refresh: Callback<()>,
}

#[function_component(AddForm)]
pub fn add_form(props: &AddFormProps) -> Html {
    let description = use_state(String::new);

    let on_submit = {
        let token = props.token.clone();
        let path = format!("/api/repos/{}/task", props.repo_id);
        let description = description.clone();
        let on_refresh = props.on_refresh.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let desc = (*description).clone();
            if desc.trim().is_empty() {
                return;
            }
            let (token, path) = (token.clone(), path.clone());
            let on_refresh = on_refresh.clone();
            spawn_local(async move {
                let body = serde_json::json!({ "description": desc });
                api_post(&token, &path, &body).await;
                on_refresh.emit(());
            });
        })
    };

    html! {
        <form class="add" onsubmit={on_submit}>
            <input
                type="text"
                class="input input-bordered"
                placeholder="new task (goes to the backlog)…"
                value={(*description).clone()}
                oninput={Callback::from(move |e: InputEvent| description.set(input_value(e)))}
            />
            <button type="submit" class="btn btn-primary">{ "add" }</button>
        </form>
    }
}

// ---------- login ----------

#[derive(Properties, PartialEq)]
pub struct LoginProps {
    pub on_login: Callback<String>,
}

#[function_component(Login)]
pub fn login(props: &LoginProps) -> Html {
    let username = use_state(String::new);
    let password = use_state(String::new);
    let error = use_state(|| false);

    let on_submit = {
        let username = username.clone();
        let password = password.clone();
        let error = error.clone();
        let on_login = props.on_login.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let (u, p) = ((*username).clone(), (*password).clone());
            let error = error.clone();
            let on_login = on_login.clone();
            spawn_local(async move {
                match api_login(u, p).await {
                    Some(token) => on_login.emit(token),
                    None => error.set(true),
                }
            });
        })
    };

    html! {
        <form class="login card bg-base-100 shadow-xl" onsubmit={on_submit}>
            <h1>{ "tod" }</h1>
            <input
                type="text"
                class="input input-bordered w-full"
                placeholder="username"
                value={(*username).clone()}
                oninput={Callback::from(move |e: InputEvent| username.set(input_value(e)))}
            />
            <input
                type="password"
                class="input input-bordered w-full"
                placeholder="password"
                value={(*password).clone()}
                oninput={Callback::from(move |e: InputEvent| password.set(input_value(e)))}
            />
            if *error {
                <p class="error">{ "invalid username or password" }</p>
            }
            <button type="submit" class="btn btn-primary w-full">{ "log in" }</button>
        </form>
    }
}

// ---------- app ----------

#[function_component(App)]
pub fn app() -> Html {
    let token = use_state(stored_token);
    let repos = use_state(Vec::<RepoView>::new);
    let status = use_state(Status::default);
    let error = use_state(String::new);
    let route = use_state(current_route);
    // the task whose detail overlay is open, by identifier
    let selected = use_state(|| None::<String>);
    // the new-repository form state, used by the creation page
    let new_repo_name = use_state(String::new);
    let new_repo_url = use_state(String::new);

    // keep the route in sync with the URL hash
    {
        let route = route.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window().expect("no window");
            let handle = route.clone();
            let on_hash_change = Closure::<dyn FnMut()>::new(move || {
                handle.set(current_route());
            });
            window
                .add_event_listener_with_callback(
                    "hashchange",
                    on_hash_change.as_ref().unchecked_ref(),
                )
                .expect("cannot listen to hashchange");
            move || {
                let _ = window.remove_event_listener_with_callback(
                    "hashchange",
                    on_hash_change.as_ref().unchecked_ref(),
                );
            }
        });
    }

    let refresh_repos = {
        let token = token.clone();
        let repos = repos.clone();
        Callback::from(move |_| {
            let t = match (*token).clone() {
                Some(t) => t,
                None => return,
            };
            let repos = repos.clone();
            let token = token.clone();
            spawn_local(async move {
                match api_get::<Vec<RepoView>>(&t, "/api/repos").await {
                    ApiOutcome::Ok(list) => repos.set(list),
                    ApiOutcome::Unauthorized => {
                        // the token is no longer valid: log out
                        clear_token();
                        navigate("#/");
                        token.set(None);
                    }
                    ApiOutcome::Failed => {}
                }
            });
        })
    };

    // fetch the status of the repository shown by the route
    let refresh_status = {
        let token = token.clone();
        let route = route.clone();
        let status = status.clone();
        Callback::from(move |_| {
            let Route::Repo(id) = *route else {
                return;
            };
            let Some(t) = (*token).clone() else {
                return;
            };
            let status = status.clone();
            let token = token.clone();
            spawn_local(async move {
                match api_get::<Status>(&t, &format!("/api/repos/{id}/status")).await {
                    ApiOutcome::Ok(s) => status.set(s),
                    ApiOutcome::Unauthorized => {
                        clear_token();
                        navigate("#/");
                        token.set(None);
                    }
                    ApiOutcome::Failed => {}
                }
            });
        })
    };

    // refetch when the route points at a repository
    {
        let refresh_status = refresh_status.clone();
        use_effect_with((*route).clone(), move |_| {
            refresh_status.emit(());
            || {}
        });
    }

    // restore the session after a page refresh
    {
        let refresh_repos = refresh_repos.clone();
        use_effect_with((), move |_| {
            refresh_repos.emit(());
            || {}
        });
    }

    let on_logout = {
        let token = token.clone();
        Callback::from(move |_| {
            clear_token();
            navigate("#/");
            token.set(None);
        })
    };

    match (*token).clone() {
        None => html! { <Login on_login={Callback::from(move |t: String| {
            save_token(&t);
            token.set(Some(t));
            refresh_repos.emit(());
            navigate("#/");
        })} /> },
        Some(t) => {
            let s = (*status).clone();

            let page = match (*route).clone() {
                Route::Repos => {
                    let list = (*repos)
                        .iter()
                        .map(|r| {
                            let detail = if r.url.is_empty() {
                                "created here".to_string()
                            } else {
                                r.url.clone()
                            };
                            html! {
                                <li>
                                    <a class="repo-card" href={format!("#/repo/{}", r.id)}>
                                        <span class="repo-name">{ r.name.clone() }</span>
                                        <span class="repo-detail">{ detail }</span>
                                    </a>
                                </li>
                            }
                        })
                        .collect::<Html>();
                    if repos.is_empty() {
                        html! {
                            <section class="empty">
                                <p>{ "no repository yet" }</p>
                                <a class="btn btn-primary" href="#/new">{ "create one" }</a>
                            </section>
                        }
                    } else {
                        html! { <ul class="repos">{ list }</ul> }
                    }
                }
                Route::NewRepo => {
                    let on_create = {
                        let t = t.clone();
                        let token = token.clone();
                        let name = new_repo_name.clone();
                        let url = new_repo_url.clone();
                        let error = error.clone();
                        let refresh_repos = refresh_repos.clone();
                        Callback::from(move |e: SubmitEvent| {
                            e.prevent_default();
                            let (n, u) = ((*name).clone(), (*url).clone());
                            if n.trim().is_empty() {
                                return;
                            }
                            // An empty url creates a new repository on the
                            // server (like GitHub); a filled one registers
                            // an existing one.
                            let (t, token, name, url, error, refresh) = (
                                t.clone(),
                                token.clone(),
                                name.clone(),
                                url.clone(),
                                error.clone(),
                                refresh_repos.clone(),
                            );
                            spawn_local(async move {
                                let body = serde_json::json!({ "name": n, "url": u });
                                match api_post_json::<RepoView>(&t, "/api/repos", &body).await {
                                    ApiOutcome::Ok(repo) => {
                                        name.set(String::new());
                                        url.set(String::new());
                                        error.set(String::new());
                                        refresh.emit(());
                                        navigate(&format!("#/repo/{}", repo.id));
                                    }
                                    ApiOutcome::Unauthorized => {
                                        clear_token();
                                        navigate("#/");
                                        token.set(None);
                                    }
                                    ApiOutcome::Failed => {
                                        error.set(
                                            "cannot create repository (name already taken or bad URL?)"
                                                .to_string(),
                                        );
                                    }
                                }
                            });
                        })
                    };

                    html! {
                        <section class="create-form card bg-base-100 shadow-xl">
                            <h2>{ "new repository" }</h2>
                            <form onsubmit={on_create}>
                                <input
                                    type="text"
                                    class="input input-bordered w-full"
                                    placeholder="repository name"
                                    value={(*new_repo_name).clone()}
                                    oninput={Callback::from(move |e: InputEvent| new_repo_name.set(input_value(e)))}
                                />
                                <input
                                    type="text"
                                    class="input input-bordered w-full"
                                    placeholder="git URL or local path (empty = create here)"
                                    value={(*new_repo_url).clone()}
                                    oninput={Callback::from(move |e: InputEvent| new_repo_url.set(input_value(e)))}
                                />
                                <button type="submit" class="btn btn-primary w-full">{ "create repository" }</button>
                            </form>
                            if !(*error).is_empty() {
                                <p class="error">{ (*error).clone() }</p>
                            }
                        </section>
                    }
                }
                Route::Account => html! { <AccountPage token={t.clone()} /> },
                Route::Repo(id) => {
                    let on_refresh = {
                        let refresh_status = refresh_status.clone();
                        Callback::from(move |_| refresh_status.emit(()))
                    };

                    let on_open = {
                        let selected = selected.clone();
                        Callback::from(move |task: TaskView| selected.set(Some(task.uid.clone())))
                    };
                    let on_close = {
                        let selected = selected.clone();
                        Callback::from(move |_| selected.set(None))
                    };

                    // one kanban column per state: a title with the
                    // card count, and the task cards
                    let column = |title: String, tasks: Vec<TaskView>| -> Html {
                        let count = tasks.len();
                        let cards = tasks
                            .iter()
                            .map(|task| html! { <TaskCard task={task.clone()} on_open={on_open.clone()} /> })
                            .collect::<Html>();
                        html! {
                            <section class="kanban-col card bg-base-200">
                                <h2>
                                    { title }
                                    <span class="count badge badge-sm badge-ghost">{ count }</span>
                                </h2>
                                <ul>{ cards }</ul>
                            </section>
                        }
                    };

                    let columns = s
                        .states
                        .iter()
                        .map(|state| column(state.name.clone(), state.tasks.clone()))
                        .collect::<Html>();

                    // the overlay task is looked up in the fresh status on
                    // every render: a checked step updates it live
                    let detail = (*selected).clone().and_then(|uid| find_task(&s, &uid));

                    // the clone address of the repository shown
                    let clone_url = (*repos)
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| r.clone_url.clone())
                        .unwrap_or_default();

                    html! {
                        <>
                            <div class="bar">
                                <p class="meta">{ format!("{} — branch {}", if s.repo.is_empty() { "?" } else { s.repo.as_str() }, s.branch) }</p>
                                if !clone_url.is_empty() {
                                    <CloneBar clone_url={clone_url} />
                                }
                                <button class="btn btn-sm" onclick={on_refresh}>{ "refresh" }</button>
                            </div>
                            <AddForm token={t.clone()} repo_id={id} on_refresh={refresh_status.clone()} />
                            <div class="board">
                                { columns }
                            </div>
                            if let Some(task) = detail {
                                <TaskDetail token={t.clone()} repo_id={id} task={task} {on_close} on_refresh={refresh_status.clone()} />
                            }
                        </>
                    }
                }
            };

            html! {
                <main>
                    <header class="site-header">
                        <a class="brand" href="#/">{ "tod" }</a>
                        <nav>
                            <a href="#/">{ "repositories" }</a>
                            <a href="#/new">{ "new repository" }</a>
                            <a href="#/account">{ "account" }</a>
                        </nav>
                        <button class="btn btn-ghost btn-sm" onclick={on_logout}>{ "log out" }</button>
                    </header>
                    { page }
                </main>
            }
        }
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
