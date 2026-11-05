use google_cloud_storage::builder::storage::SignedUrlBuilder;
use google_cloud_storage::http::Method;
use std::time::Duration;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{Json, Router};
use axum::response::IntoResponse;
use axum::routing::post;
use serde::{Deserialize};
use serde_json::json;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::GovernorLayer;
use crate::state::AppState;

// pub fn router() -> Router<AppState> {
//     let rate_limit = GovernorConfigBuilder::default()
//         .per_second(6)
//         .burst_size(10)
//         .finish()
//         .unwrap();
//     Router::new().route("", post(create_url)).layer(GovernorLayer::new(rate_limit))
// }

#[derive(Deserialize, Debug, Clone)]
pub struct UrlCreationPayload {
    bucket_name: String,
    object_name: String,
}

// pub async fn create_url(
//     State(state): State<AppState>,
//     _auth: crate::extractors::auth::AuthUser,
//     Json(payload): Json<UrlCreationPayload>
// ) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
//     let url = sample(&payload.bucket_name, &payload.object_name)
//         .await
//         .map_err(|err| {
//             eprintln!("Error while creating signed url:  {:?}", err);
//             (
//                 StatusCode::INTERNAL_SERVER_ERROR,
//                 Json(json!({
//                     "status": "Error",
//                     "message": "Internal Server Error"
//                 }))
//                 )
//         })?;
// 
//     Ok((StatusCode::CREATED, Json(json!({"status": "success", "url": url}))))
// }

pub async fn sample(
    bucket_name: &str,
    username: &str,
    filename: &str,
) -> anyhow::Result<String> {
    let signer = google_cloud_auth::credentials::Builder::default()
        .build_signer()?;

    let object_name = format!("user/{username}/{filename}");

    let signed_url = SignedUrlBuilder::for_object(
        format!("projects/_/buckets/{bucket_name}"),
        &object_name,
    )
        .with_method(Method::PUT)
        .with_expiration(Duration::from_secs(15 * 60))
        .with_header("content-type", "text/markdown")
        .sign_with(&signer)
        .await?;

    Ok(signed_url)
}