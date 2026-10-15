//! SSH public keys model: the keys that authenticate the git clients.

use loco_rs::prelude::*;

pub use super::_entities::user_keys::{self, ActiveModel, Entity, Model};

/// The canonical text of a public key: openssh format without the
/// comment, so that a registered key ("ssh-ed25519 AAAA… user@laptop")
/// and the key presented during authentication (no comment on the
/// wire) compare equal.
pub fn canonical_key(key: &russh::keys::ssh_key::PublicKey) -> loco_rs::Result<String> {
    let mut key = key.clone();
    key.set_comment("");
    Ok(key.to_openssh().map_err(loco_rs::Error::msg)?)
}

/// Register a public key (canonical openssh text) for a user.
pub async fn add(db: &DatabaseConnection, user_id: i64, public_key: &str) -> ModelResult<Model> {
    let key = user_keys::ActiveModel {
        user_id: sea_orm::Set(user_id),
        public_key: sea_orm::Set(public_key.to_string()),
        ..Default::default()
    };
    Ok(key.insert(db).await?)
}

/// The keys of a user.
pub async fn list(db: &DatabaseConnection, user_id: i64) -> ModelResult<Vec<Model>> {
    Ok(user_keys::Entity::find()
        .filter(user_keys::Column::UserId.eq(user_id))
        .all(db)
        .await?)
}

/// Delete one of the user's own keys. Returns false when the key
/// does not exist or belongs to someone else.
pub async fn delete(db: &DatabaseConnection, user_id: i64, id: i64) -> ModelResult<bool> {
    Ok(user_keys::Entity::delete_many()
        .filter(user_keys::Column::Id.eq(id))
        .filter(user_keys::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected
        > 0)
}

/// Who owns a given public key? The key alone identifies the user
/// (the git convention: any login name, git@...).
pub async fn key_owner(db: &DatabaseConnection, public_key: &str) -> ModelResult<Option<String>> {
    use super::_entities::users;

    let Some(key) = user_keys::Entity::find()
        .filter(user_keys::Column::PublicKey.eq(public_key))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let owner = users::Entity::find_by_id(key.user_id)
        .one(db)
        .await?
        .map(|user| user.email);
    Ok(owner)
}
