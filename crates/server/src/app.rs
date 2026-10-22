use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Hooks, Initializer},
    bgworker::{BackgroundWorker, Queue},
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    db::{self, truncate_table},
    environment::Environment,
    task::Tasks,
    Result,
};
use axum::Router as AxumRouter;
use migration::Migrator;
use std::path::Path;

#[allow(unused_imports)]
use crate::{controllers, models::_entities::users, tasks, workers::downloader::DownloadWorker};

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![])
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes() // controller routes below
            .add_route(controllers::auth::routes())
            .add_route(controllers::auth::tod_routes())
            .add_route(controllers::account::routes())
            .add_route(controllers::repos::routes())
            .add_route(controllers::keys::routes())
    }

    /// Boot the embedded git-over-SSH server alongside the HTTP
    /// server. The port binds here so a busy port fails loudly.
    async fn after_routes(router: AxumRouter, ctx: &AppContext) -> Result<AxumRouter> {

        let settings = crate::settings::Settings::from_ctx(ctx)?;
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", settings.ssh_port))
            .await
            .map_err(|e| {
                loco_rs::Error::string(&format!(
                    "cannot bind ssh port {}: {e}",
                    settings.ssh_port
                ))
            })?;
        let port = listener.local_addr()?.port();
        let state = crate::ssh::SshState {
            db: ctx.db.clone(),
            workdir: std::path::PathBuf::from(&settings.workdir),
        };
        let ssh_config = crate::ssh::config(std::path::Path::new(&settings.workdir))
            .map_err(|e| loco_rs::Error::string(&format!("{e:#}")))?;
        tokio::spawn(crate::ssh::serve_on(state, ssh_config, listener));
        println!("git over ssh on port {port}: git clone ssh://git@localhost:{port}/<repo>.git");

        // serve the built web console as the fallback of the router
        let static_dir = std::path::PathBuf::from(&settings.static_dir);
        Ok(router.fallback_service(tower_http::services::ServeDir::new(
            &static_dir,
        )))
    }
    async fn connect_workers(ctx: &AppContext, queue: &Queue) -> Result<()> {
        queue.register(DownloadWorker::build(ctx)).await?;
        Ok(())
    }

    #[allow(unused_variables)]
    fn register_tasks(tasks: &mut Tasks) {
        // tasks-inject (do not remove)
        tasks.register(tasks::user_create::UserCreate);
        tasks.register(tasks::user_delete::UserDelete);
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
    async fn seed(ctx: &AppContext, base: &Path) -> Result<()> {
        db::seed::<users::ActiveModel>(&ctx.db, &base.join("users.yaml").display().to_string())
            .await?;
        Ok(())
    }
}
