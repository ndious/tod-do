//! SSH key controller: register the public keys that authenticate
//! the git clients.

use crate::models::{user_keys, users};
use loco_rs::prelude::*;
use loco_rs::controller::extractor::auth::JWTWithUser;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AddKeyParams {
    pub public_key: String,
}

/// Register a key for the logged-in user. The key is stored in
/// canonical form (no comment) so the SSH server can compare it
/// with the wire form directly.
#[debug_handler]
async fn add(
    auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Json(params): Json<AddKeyParams>,
) -> Result<Response> {
    let key = russh::keys::ssh_key::PublicKey::from_openssh(params.public_key.trim())
        .map_err(|_| Error::BadRequest("invalid public key".into()))?;
    let normalized = crate::models::user_keys::canonical_key(&key)
        .map_err(|_| Error::BadRequest("invalid public key".into()))?;
    // one key belongs to exactly one user
    if user_keys::key_owner(&ctx.db, &normalized).await?.is_some() {
        return Err(Error::BadRequest("this key is already registered".into()));
    }
    let key = user_keys::add(&ctx.db, auth.user.id, &normalized).await?;
    format::json(key)
}

/// List the keys of the logged-in user.
#[debug_handler]
async fn list(
    auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let keys = user_keys::list(&ctx.db, auth.user.id).await?;
    format::json(keys)
}

/// Delete one of the logged-in user's keys.
#[debug_handler]
async fn delete_key(
    auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
) -> Result<Response> {
    if user_keys::delete(&ctx.db, auth.user.id, id).await? {
        format::empty()
    } else {
        Err(Error::NotFound)
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/keys")
        .add("/", get(list))
        .add("/", post(add))
        .add("/{id}", delete(delete_key))
}
