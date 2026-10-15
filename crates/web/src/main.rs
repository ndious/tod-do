//! tod web console — Yew (CSR) app: login, repository registry,
//! and the task board of each repository. Served by the tod server.

use gloo_net::http::Request;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
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
    pub done: bool,
    pub context: String,
    pub plan: Vec<StepView>,
    pub progress: (usize, usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextView {
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
    pub backlog: Vec<TaskView>,
    #[serde(default)]
    pub contexts: Vec<ContextView>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RepoView {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
}

// ---------- api helpers ----------

async fn api_get<T: for<'de> Deserialize<'de>>(token: &str, path: &str) -> Option<T> {
    let resp = Request::get(path)
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .ok()?;
    resp.json::<T>().await.ok()
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

// ---------- components ----------

#[derive(Properties, PartialEq)]
pub struct TaskCardProps {
    pub token: String,
    pub repo_id: i64,
    pub task: TaskView,
    pub on_refresh: Callback<()>,
}

#[function_component(TaskCard)]
pub fn task_card(props: &TaskCardProps) -> Html {
    let task = &props.task;
    let new_step = use_state(String::new);

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
            let mark = if s.done { "[x]" } else { "[ ]" };
            html! { <li class={class} {onclick}>{ format!("{mark} {}", s.text) }</li> }
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

    let class = if task.done { "task done" } else { "task" };
    let short = &task.uid[..task.uid.len().min(7)];

    html! {
        <li class={class}>
            <span class="desc">{ task.description.clone() }</span>
            <span class="uid">{ short }</span>
            if !task.plan.is_empty() {
                <span class="progress">{ format!("{}/{}", task.progress.0, task.progress.1) }</span>
            }
            if !task.plan.is_empty() {
                <ol class="plan">{ steps }</ol>
            }
            <form class="sub" onsubmit={on_submit_step}>
                <input
                    type="text"
                    placeholder="new micro todo…"
                    value={(*new_step).clone()}
                    oninput={Callback::from(move |e: InputEvent| new_step.set(input_value(e)))}
                />
                <button type="submit">{ "+" }</button>
            </form>
        </li>
    }
}

#[derive(Properties, PartialEq)]
pub struct AddFormProps {
    pub token: String,
    pub repo_id: i64,
    pub contexts: Vec<String>,
    pub on_refresh: Callback<()>,
}

#[function_component(AddForm)]
pub fn add_form(props: &AddFormProps) -> Html {
    let description = use_state(String::new);
    let context = use_state(|| "backlog".to_string());

    let on_submit = {
        let token = props.token.clone();
        let path = format!("/api/repos/{}/task", props.repo_id);
        let description = description.clone();
        let context = context.clone();
        let on_refresh = props.on_refresh.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let desc = (*description).clone();
            if desc.trim().is_empty() {
                return;
            }
            let ctx = (*context).clone();
            let (token, path) = (token.clone(), path.clone());
            let on_refresh = on_refresh.clone();
            spawn_local(async move {
                let body = serde_json::json!({
                    "description": desc,
                    "context": if ctx == "backlog" {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(ctx)
                    },
                });
                api_post(&token, &path, &body).await;
                on_refresh.emit(());
            });
        })
    };

    let options = std::iter::once("backlog".to_string())
        .chain(props.contexts.iter().cloned())
        .map(|c| {
            let selected = (*context).clone() == c;
            html! { <option value={c.clone()} {selected}>{ c }</option> }
        })
        .collect::<Html>();

    let ctx = context.clone();
    html! {
        <form class="add" onsubmit={on_submit}>
            <input
                type="text"
                placeholder="new task…"
                value={(*description).clone()}
                oninput={Callback::from(move |e: InputEvent| description.set(input_value(e)))}
            />
            <select oninput={Callback::from(move |e: InputEvent| ctx.set(select_value(e)))}>
                { options }
            </select>
            <button type="submit">{ "add" }</button>
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
        <form class="login" onsubmit={on_submit}>
            <h1>{ "tod" }</h1>
            <input
                type="text"
                placeholder="username"
                value={(*username).clone()}
                oninput={Callback::from(move |e: InputEvent| username.set(input_value(e)))}
            />
            <input
                type="password"
                placeholder="password"
                value={(*password).clone()}
                oninput={Callback::from(move |e: InputEvent| password.set(input_value(e)))}
            />
            if *error {
                <p class="error">{ "invalid username or password" }</p>
            }
            <button type="submit">{ "log in" }</button>
        </form>
    }
}

// ---------- app ----------

#[function_component(App)]
pub fn app() -> Html {
    let token = use_state(|| Option::<String>::None);
    let repos = use_state(Vec::<RepoView>::new);
    let current = use_state(|| Option::<i64>::None);
    let status = use_state(Status::default);
    let error = use_state(String::new);
    // all hooks must live at the top level of the component, even
    // the ones only used once logged in
    let new_repo_name = use_state(String::new);
    let new_repo_url = use_state(String::new);

    let refresh_repos = {
        let token = token.clone();
        let repos = repos.clone();
        Callback::from(move |_| {
            let t = match (*token).clone() {
                Some(t) => t,
                None => return,
            };
            let repos = repos.clone();
            spawn_local(async move {
                if let Some(list) = api_get::<Vec<RepoView>>(&t, "/api/repos").await {
                    repos.set(list);
                }
            });
        })
    };

    let refresh_status = {
        let token = token.clone();
        let current = current.clone();
        let status = status.clone();
        Callback::from(move |_| {
            let (t, id) = match ((*token).clone(), *current) {
                (Some(t), Some(id)) => (t, id),
                _ => return,
            };
            let status = status.clone();
            spawn_local(async move {
                if let Some(s) = api_get::<Status>(&t, &format!("/api/repos/{id}/status")).await {
                    status.set(s);
                }
            });
        })
    };

    {
        let refresh_status = refresh_status.clone();
        use_effect_with(*current, move |_| {
            refresh_status.emit(());
            || {}
        });
    }

    match (*token).clone() {
        None => html! { <Login on_login={Callback::from(move |t: String| {
            token.set(Some(t));
            refresh_repos.emit(());
        })} /> },
        Some(t) => {
            let s = (*status).clone();

            let repo_options = (*repos)
                .iter()
                .map(|r| {
                    let selected = *current == Some(r.id);
                    html! { <option value={r.id.to_string()} {selected}>{ r.name.clone() }</option> }
                })
                .collect::<Html>();

            let on_select_repo = {
                let current = current.clone();
                Callback::from(move |e: InputEvent| {
                    let v = select_value(e);
                    if let Ok(id) = v.parse::<i64>() {
                        current.set(Some(id));
                    }
                })
            };

            let on_add_repo = {
                let t = t.clone();
                let name = new_repo_name.clone();
                let url = new_repo_url.clone();
                let error = error.clone();
                let refresh_repos = refresh_repos.clone();
                Callback::from(move |e: SubmitEvent| {
                    e.prevent_default();
                    let (n, u) = ((*name).clone(), (*url).clone());
                    if n.trim().is_empty() || u.trim().is_empty() {
                        return;
                    }
                    let (t, error, refresh) = (t.clone(), error.clone(), refresh_repos.clone());
                    spawn_local(async move {
                        let body = serde_json::json!({ "name": n, "url": u });
                        if api_post(&t, "/api/repos", &body).await {
                            error.set(String::new());
                            refresh.emit(());
                        } else {
                            error.set("cannot add repository (bad URL, or no .tod inside?)".to_string());
                        }
                    });
                })
            };

            let ctx_names: Vec<String> = s.contexts.iter().map(|c| c.name.clone()).collect();

            // the task board only exists once a repository is selected
            let board = match *current {
                Some(id) => {
                    let backlog = s
                        .backlog
                        .iter()
                        .map(|task| html! { <TaskCard token={t.clone()} repo_id={id} task={task.clone()} on_refresh={refresh_status.clone()} /> })
                        .collect::<Html>();
                    let contexts = s
                        .contexts
                        .iter()
                        .map(|c| {
                            let tasks = c
                                .tasks
                                .iter()
                                .map(|task| html! { <TaskCard token={t.clone()} repo_id={id} task={task.clone()} on_refresh={refresh_status.clone()} /> })
                                .collect::<Html>();
                            html! {
                                <section class="context">
                                    <h2>{ format!("context: {}", c.name) }</h2>
                                    <ul>{ tasks }</ul>
                                </section>
                            }
                        })
                        .collect::<Html>();
                    html! {
                        <>
                            <AddForm token={t.clone()} repo_id={id} contexts={ctx_names} on_refresh={refresh_status.clone()} />
                            <section class="backlog">
                                <h2>{ "backlog" }</h2>
                                <ul>{ backlog }</ul>
                            </section>
                            { contexts }
                        </>
                    }
                }
                None => html! {},
            };

            html! {
                <main>
                    <header>
                        <h1>{ "tod" }</h1>
                        <p class="meta">{ format!("{} — branch {}", if s.repo.is_empty() { "?" } else { s.repo.as_str() }, s.branch) }</p>
                    </header>

                    <div class="bar">
                        <select oninput={on_select_repo}>
                            if repos.is_empty() {
                                <option>{ "no repository yet" }</option>
                            }
                            { repo_options }
                        </select>
                        <button onclick={Callback::from(move |_| refresh_status.emit(()))}>{ "refresh" }</button>
                    </div>

                    <form class="bar" onsubmit={on_add_repo}>
                        <input
                            type="text"
                            placeholder="repo name"
                            value={(*new_repo_name).clone()}
                            oninput={Callback::from(move |e: InputEvent| new_repo_name.set(input_value(e)))}
                        />
                        <input
                            type="text"
                            placeholder="git URL or local path"
                            value={(*new_repo_url).clone()}
                            oninput={Callback::from(move |e: InputEvent| new_repo_url.set(input_value(e)))}
                        />
                        <button type="submit">{ "add repo" }</button>
                    </form>
                    if !(*error).is_empty() {
                        <p class="meta">{ (*error).clone() }</p>
                    }

                    { board }
                </main>
            }
        }
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
