use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx;
use uuid::Uuid;
use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHasher,
        SaltString,
    },
    Argon2,
};

use tower_governor::{
    governor::GovernorConfigBuilder,
    key_extractor::SmartIpKeyExtractor,
    GovernorLayer,
};

pub fn router() -> Router<AppState> {
    let rate_limit = GovernorConfigBuilder::default()
        .per_second(30)
        .burst_size(2)
        .key_extractor(SmartIpKeyExtractor)
        .finish()
        .unwrap();

    Router::new()
        .route("/auth/register", post(register))
        .layer(GovernorLayer::new(rate_limit))
}



#[derive(Deserialize, Clone, Debug)]
pub struct RegisterInfo {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, sqlx::FromRow)]
pub struct RegisterSchema{
    pub id: Uuid,
    pub username: String,
    pub password: String,
}
pub fn hash_password(
    password: &str,
) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string();

    Ok(password_hash)
}

async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterInfo>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let password_hash = hash_password(&payload.password).map_err(|err| {
        eprintln!("Password hashing failed: {err}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": "Internal Server Error"})),
        )
    })?;

    sqlx::query(
        "INSERT INTO users (username, password) VALUES ($1, $2)"
    )
        .bind(&payload.username)
        .bind(&password_hash)
        .execute(&state.database)
        .await
        .map_err(|err| {
            eprintln!("Error registering user: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": "Internal Server Error"})),
            )
        })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({"status": "success"})),
    ))
}



