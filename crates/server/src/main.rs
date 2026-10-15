//! tod-server: multi-repo project management server on top of the
//! dot store. Login/password (SQLite), git clone/pull, JSON API and
//! the Yew web console.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;

mod api;
mod db;
mod repos;
mod ssh;

#[derive(Parser, Debug)]
#[command(name = "tod-server", about = "Multi-repo project management server for .tod repositories")]
struct Args {
    /// Port to listen on
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// SQLite database file
    #[arg(long, default_value = "tod-server.db")]
    db: PathBuf,

    /// Directory where remote repositories are cloned
    #[arg(long, default_value = ".tod-server")]
    workdir: PathBuf,

    /// Directory holding the built web console (crates/web/dist)
    #[arg(long, default_value = "dist")]
    static_dir: PathBuf,

    /// Port of the embedded git-over-SSH server
    #[arg(long, default_value_t = 2222)]
    ssh_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let database = db::Db::open(&args.db)?;
    database.bootstrap_from_env()?;

    let state = Arc::new(api::AppState {
        db: database,
        workdir: args.workdir.clone(),
    });

    // git over SSH: bind now so a busy port fails loudly at startup
    let ssh_listener = tokio::net::TcpListener::bind(("0.0.0.0", args.ssh_port))
        .await
        .with_context(|| format!("cannot bind ssh port {}", args.ssh_port))?;
    let ssh_port = ssh_listener.local_addr()?.port();
    {
        let state = state.clone();
        let ssh_config = ssh::config(&args.workdir)?;
        tokio::spawn(async move {
            if let Err(e) = ssh::serve_on(state, ssh_config, ssh_listener).await {
                eprintln!("ssh server error: {e}");
            }
        });
    }

    let router = api::router(state).fallback_service(
        tower_http::services::ServeDir::new(&args.static_dir),
    );

    let addr = format!("0.0.0.0:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("cannot bind {addr}"))?;
    println!(
        "tod-server listening on http://{addr} (db: {}, workdir: {}, static: {})",
        args.db.display(),
        args.workdir.display(),
        args.static_dir.display()
    );
    println!("git over ssh on port {ssh_port}: git clone ssh://git@localhost:{ssh_port}/<repo>.git");
    axum::serve(listener, router).await?;
    Ok(())
}
