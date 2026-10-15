//! Tod-specific settings, read from the `settings` section of the
//! loco configuration file.

use loco_rs::app::AppContext;
use serde::Deserialize;
use std::path::PathBuf;

fn default_workdir() -> String {
    ".tod-server".to_string()
}

fn default_ssh_port() -> u16 {
    2222
}

fn default_static_dir() -> String {
    "dist".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// Where the repository mirrors and the SSH host key live.
    #[serde(default = "default_workdir")]
    pub workdir: String,
    /// Port of the embedded git-over-SSH server.
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    /// Directory holding the built web console.
    #[serde(default = "default_static_dir")]
    pub static_dir: String,
}

impl Settings {
    pub fn from_ctx(ctx: &AppContext) -> loco_rs::Result<Self> {
        let mut settings: Settings = ctx
            .config
            .settings
            .clone()
            .map(serde_json::from_value::<Settings>)
            .transpose()
            .map_err(loco_rs::Error::msg)?
            .ok_or_else(|| loco_rs::Error::string("no settings section in the configuration"))?;

        // git plumbing resolves relative paths from unexpected
        // directories (git -C): the workdir must be absolute
        let workdir = PathBuf::from(&settings.workdir);
        if !workdir.is_absolute() {
            let cwd = std::env::current_dir().map_err(loco_rs::Error::msg)?;
            settings.workdir = cwd.join(workdir).to_string_lossy().into_owned();
        }
        Ok(settings)
    }
}
