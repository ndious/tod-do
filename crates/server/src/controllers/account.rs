//! Account controller: the logged-in user — their profile and their
//! password. The SSH keys live in their own controller (`keys`).

use crate::models::users;
use loco_rs::controller::extractor::auth::JWTWithUser;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct AccountView {
    pub email: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PasswordParams {
    pub current: String,
    pub new: String,
}

/// The account of the logged-in user.
#[debug_handler]
async fn show(
    auth: JWTWithUser<users::Model>,
    State(_ctx): State<AppContext>,
) -> Result<Response> {
    format::json(AccountView {
        email: auth.user.email,
        name: auth.user.name,
    })
}

/// Change the password: the current one is verified first, and the
/// new one must be at least 8 characters.
#[debug_handler]
async fn password(
    auth: JWTWithUser<users::Model>,
    State(ctx): State<AppContext>,
    Json(params): Json<PasswordParams>,
) -> Result<Response> {
    if !auth.user.verify_password(&params.current) {
        return Err(Error::BadRequest("wrong current password".into()));
    }
    if params.new.len() < 8 {
        return Err(Error::BadRequest(
            "the new password is too short (8 characters minimum)".into(),
        ));
    }
    users::Model::set_password(&ctx.db, auth.user.id, &params.new).await?;
    format::empty()
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/account")
        .add("/", get(show))
        .add("/password", post(password))
}
