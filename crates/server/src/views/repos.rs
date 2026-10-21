//! Serializations of the repository registry for the API.

use crate::models::repos;

#[derive(Debug, serde::Serialize)]
pub struct RepoView {
    pub id: i64,
    pub name: String,
    pub url: String,
    /// How a client clones this repository from the server, over SSH.
    pub clone_url: String,
}

impl RepoView {
    /// Build the view of a repository: the registry entry plus the
    /// SSH clone URL, from the host the client addressed and the SSH
    /// port of the server settings.
    pub fn of(repo: &repos::Model, host: &str, ssh_port: u16) -> Self {
        Self {
            id: repo.id,
            name: repo.name.clone(),
            url: repo.url.clone(),
            clone_url: format!("ssh://git@{host}:{ssh_port}/{}.git", repo.name),
        }
    }
}
