use crate::routes::auth::register::RegisterSchema;
use crate::state::AppState;
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use axum::routing::post;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx;
use tower_governor::{
    governor::GovernorConfigBuilder,
    key_extractor::SmartIpKeyExtractor,
    GovernorLayer,
};
use uuid::Uuid;

#[derive(Deserialize, Debug, Clone)]
pub struct LoginInfo {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, sqlx::FromRow)]
pub struct TokensSchema {
    pub id: Uuid,
    pub username: String,
    pub token: Uuid,
    pub expiring: DateTime<Utc>,
}



pub fn router() -> Router<AppState> {
    let rate_limit = GovernorConfigBuilder::default()
        .per_second(5)
        .burst_size(12)
        .key_extractor(SmartIpKeyExtractor)
        .finish()
        .unwrap();

    Router::new()
        .route("/auth/login", post(login))
        .layer(GovernorLayer::new(rate_limit))
}

pub fn verify_password(
    password: &str,
    stored_hash: &str,
) -> Result<bool, argon2::password_hash::Error> {
    let parsed_hash = PasswordHash::new(stored_hash)?;

    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(err) => Err(err),
    }
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginInfo>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let get_hash = sqlx::query_as::<_, RegisterSchema>("SELECT * FROM users WHERE username = $1;")
        .bind(&payload.username)
        .fetch_optional(&state.database)
        .await
        .map_err(|err| {
            eprintln!("Database query failed for login: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "Internal Server Error"
                })),
            )
        })?;
    return match get_hash {
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"status": "error", "message": "Username Does Not Exist"})),
        )),
        Some(hash) => {
            let password_hash = hash.password;
            let answer = verify_password(&payload.password, &password_hash).map_err(|err| {
                eprintln!("Password Verify Failed: {:?}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "status": "error",
                        "message": "Internal Server Error"
                    })),
                )
            })?;
            match answer {
                false => Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "invalid_credentials",
                    "message": "The username or password you entered is incorrect."
                    })),
                )),
                true => {
                    let query = sqlx::query_as::<_, TokensSchema>("
                        INSERT INTO tokens (username) VALUES ($1) RETURNING *;
                    ")
                        .bind(&payload.username)
                        .fetch_one(&state.database)
                        .await
                        .map_err(|err| {
                            eprintln!("Database error while registering: {:?}", err);
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(json!({
                                    "status": "error",
                                    "message": "Internal Server Error"
                                }))
                                )
                        })?;
                    Ok((StatusCode::OK, Json(json!({
                        "status": "success",
                        "message": "Token Created",
                        "token": query.token,
                    }))))
                }
            }
        }
    };
    // let answer = verify_password(&payload.password, Some(&get_hash));
}
