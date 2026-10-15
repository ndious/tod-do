//! Auth controller: register + login (JWT). The tod server has no
//! mail flows, so the mailer-dependent endpoints of the scaffold
//! (email verification, magic link, password reset) are not wired.

use crate::{
    models::{
        _entities::users,
        users::{LoginParams, RegisterParams},
    },
    views::auth::LoginResponse,
};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterResponse {
    pub id: i64,
}

/// Create a user directly — for now the tod server has no public
/// sign-up: users are created by bootstrap or by an admin.
#[debug_handler]
async fn register(
    State(ctx): State<AppContext>,
    Json(params): Json<RegisterParams>,
) -> Result<Response> {
    let user = users::Model::create_with_password(&ctx.db, &params).await?;
    format::json(RegisterResponse { id: user.id })
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginTodParams {
    pub username: String,
    pub password: String,
}

/// The tod contract: the web console logs in with
/// `POST /api/login {username, password}` and expects `{token}`.
/// The username is the user's email.
#[debug_handler]
async fn login_tod(
    State(ctx): State<AppContext>,
    Json(params): Json<LoginTodParams>,
) -> Result<Response> {
    let user = users::Model::find_by_email(&ctx.db, &params.username).await;

    if let Ok(user) = user {
        if user.verify_password(&params.password) {
            let jwt_secret = ctx.config.get_jwt_config()?;
            let token = user
                .generate_jwt(&jwt_secret.secret, jwt_secret.expiration)
                .or_else(|_| unauthorized("invalid username or password"))?;
            return format::json(serde_json::json!({ "token": token }));
        }
    }

    unauthorized("invalid username or password")
}

/// Log in with email and password, returning a JWT.
#[debug_handler]
async fn login(State(ctx): State<AppContext>, Json(params): Json<LoginParams>) -> Result<Response> {
    let user = users::Model::find_by_email(&ctx.db, &params.email).await;

    if let Ok(user) = user {
        let valid = user.verify_password(&params.password);

        if valid {
            let jwt_secret = ctx.config.get_jwt_config()?;

            let token = user
                .generate_jwt(&jwt_secret.secret, jwt_secret.expiration)
                .or_else(|_| unauthorized("unauthorized!"))?;

            return format::json(LoginResponse::new(&user, &token));
        }
    }

    unauthorized("unauthorized!")
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/auth")
        .add("/register", post(register))
        .add("/login", post(login))
}

/// The tod-contract endpoints, outside the /api/auth prefix.
pub fn tod_routes() -> Routes {
    Routes::new().add("/api/login", post(login_tod))
}
