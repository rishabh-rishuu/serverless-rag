use crate::state::AppState;
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthUser {
    pub username: String,
}

pub enum TokenVerifyError {
    InvalidToken,
    DatabaseError(sqlx::Error),
}

pub async fn verify_token(token_str: &str, db: &PgPool) -> Result<String, TokenVerifyError> {
    let token_uuid = Uuid::parse_str(token_str).map_err(|_| TokenVerifyError::InvalidToken)?;

    let row = sqlx::query("SELECT username, expiring FROM tokens WHERE token = $1")
        .bind(token_uuid)
        .fetch_optional(db)
        .await
        .map_err(TokenVerifyError::DatabaseError)?
        .ok_or(TokenVerifyError::InvalidToken)?;

    let expiring: DateTime<Utc> = row
        .try_get("expiring")
        .map_err(TokenVerifyError::DatabaseError)?;

    if expiring < Utc::now() {
        let _ = sqlx::query("DELETE FROM tokens WHERE token = $1")
            .bind(token_uuid)
            .execute(db)
            .await;
        return Err(TokenVerifyError::InvalidToken);
    }

    let username: String = row
        .try_get("username")
        .map_err(TokenVerifyError::DatabaseError)?;

    Ok(username)
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|val| val.to_str().ok())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "status": "error",
                        "message": "Missing Authorization header"
                    })),
                )
                    .into_response()
            })?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "status": "error",
                    "message": "Invalid authorization scheme"
                })),
            )
                .into_response()
        })?;

        match verify_token(token, &state.database).await {
            Ok(username) => Ok(AuthUser { username }),
            Err(TokenVerifyError::InvalidToken) => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "status": "error",
                    "message": "Invalid or expired session token"
                })),
            )
                .into_response()),
            Err(TokenVerifyError::DatabaseError(err)) => {
                eprintln!("Auth database failure: {:?}", err);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "status": "error",
                        "message": "Internal server error"
                    })),
                )
                    .into_response())
            }
        }
    }
}