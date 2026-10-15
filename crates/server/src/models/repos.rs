//! Repository registry model.

use loco_rs::prelude::*;

pub use super::_entities::repos::{self, ActiveModel, Entity, Model};

/// Find a repository by its unique name — how the SSH clients
/// address them: git@host:<name>.git.
pub async fn find_by_name(db: &DatabaseConnection, name: &str) -> ModelResult<Model> {
    repos::Entity::find()
        .filter(repos::Column::Name.eq(name))
        .one(db)
        .await?
        .ok_or_else(|| ModelError::EntityNotFound)
}
