use std::fs;
use std::path::Path;

use anyhow::{bail, Result};
use serde::Deserialize;

/// Name of the project configuration file, at the root of the
/// user's repository.
pub const CONFIG_FILE: &str = ".tod.toml";

/// The commented example written by `tod init` when the repository
/// has no configuration yet: every key is ready to edit, and the
/// defaults are the commented values.
pub const SCAFFOLD: &str = r#"# .tod.toml — tod's project configuration.
# Every key is optional: comment lines out to get the defaults.

[sync]
# The git remote tod syncs with: the 'tod' branch is pulled from it
# before every command and pushed to it after each mutation, and the
# review/test branches are pushed there too.
remote = "origin"

[verify]
# The command running the validation tests of the project. Executed
# by `tod verify` at the repository root, through the shell: its
# exit code becomes the command's exit code.
command = "cargo test"
"#;

/// The default sync remote, a git convention since the first clone.
pub const DEFAULT_REMOTE: &str = "origin";

/// What a project configures in `.tod.toml`: the per-repository
/// settings tod cannot guess. Every section and key is optional —
/// a missing file is a valid, empty config with the defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Which remote the todos sync with.
    pub sync: Sync,
    /// How the validation tests are run.
    pub verify: Verify,
}

impl Config {
    /// The sync remote of this project: the configured one, or the
    /// `origin` default.
    pub fn remote(&self) -> &str {
        self.sync.remote.as_deref().unwrap_or(DEFAULT_REMOTE)
    }

    /// Read the project configuration at the repository root. A
    /// missing file is the empty config; a broken file or an
    /// unknown key is an error — a typo in `.tod.toml` must not be
    /// silently ignored as if the key were absent.
    pub fn load(root: &Path) -> Result<Config> {
        let path = root.join(CONFIG_FILE);
        match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => Ok(config),
                // the reason of the failure travels with the
                // context: a one-line display must still name the
                // unknown or malformed key
                Err(e) => bail!("cannot parse {}: {e}", path.display()),
            },
            Err(_) => Ok(Config::default()),
        }
    }
}

/// Which remote the todos sync with.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sync {
    /// The git remote the tod branch syncs with (pull before every
    /// command, push after each mutation) and the review/test
    /// branches are pushed to. Default: `origin`.
    pub remote: Option<String>,
}

/// How the validation tests are run.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Verify {
    /// A shell command run by `tod verify` at the repository root.
    /// None: `tod verify` refuses, pointing at `.tod.toml`.
    pub command: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_the_default_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.remote(), "origin");
        assert_eq!(config.verify.command, None);
    }

    #[test]
    fn read_remote_and_verify_command() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE),
            "[sync]\nremote = \"todserver\"\n\n[verify]\ncommand = \"make test\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.remote(), "todserver");
        assert_eq!(config.verify.command.as_deref(), Some("make test"));
    }

    #[test]
    fn a_remoteless_file_still_means_origin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CONFIG_FILE), "[verify]\ncommand = \"x\"\n").unwrap();
        assert_eq!(Config::load(dir.path()).unwrap().remote(), "origin");
    }

    #[test]
    fn unknown_keys_are_rejected_as_typos() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE),
            "[verify]\ncomand = \"cargo test\"\n",
        )
        .unwrap();
        let error = Config::load(dir.path()).unwrap_err().to_string();
        assert!(error.contains(CONFIG_FILE), "points at the file: {error}");
        assert!(error.contains("comand"), "names the unknown key: {error}");
    }

    #[test]
    fn broken_toml_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CONFIG_FILE), "not [ valid toml").unwrap();
        let error = Config::load(dir.path()).unwrap_err().to_string();
        assert!(error.contains(CONFIG_FILE), "points at the file: {error}");
    }
}
