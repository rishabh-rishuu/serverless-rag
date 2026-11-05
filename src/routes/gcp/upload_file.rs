use crate::extractors::auth::AuthUser;
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::routing::post;
use axum::{http::StatusCode, response::IntoResponse, Json, Router};
use serde_json::{json, Value};

const MAX_FILE_SIZE: usize = 20 * 1024 * 1024;
const BUCKET_NAME: &str = "serverless-rag";

pub struct UploadedFile {
    pub filename: String,
    pub extension: String,
    pub data: Vec<u8>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/gcp/upload-file", post(upload_file))
        .layer(DefaultBodyLimit::max(MAX_FILE_SIZE))
}

pub async fn upload_file(
    State(state): State<AppState>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let mut field = multipart
        .next_field()
        .await
        .map_err(|e| {
            eprintln!("Multipart next_field error: {:?}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Failed reading field: {e}")})),
            )
        })?
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No file provided"})),
        ))?;

    let filename = field
        .file_name()
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing filename"})),
        ))?
        .to_string();

    let extension = std::path::Path::new(&filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .ok_or((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({"error": "Invalid extension"})),
        ))?;

    if !matches!(extension.as_str(), "txt" | "md") {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({"error": "Only .txt and .md files allowed"})),
        ));
    }

    let mut data = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Chunk reading failed: {e}")})),
        )
    })? {
        if data.len() + chunk.len() > MAX_FILE_SIZE {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({"error": "Maximum file size is 20 MB"})),
            ));
        }
        data.extend_from_slice(&chunk);
    }

    let file = UploadedFile {
        filename,
        extension,
        data,
    };

    let url = crate::routes::gcp::createUrl::sample(BUCKET_NAME, &auth.username, &file.filename)
        .await
        .map_err(|err| {
            eprintln!("Error creating signed URL: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": "Failed to generate upload URL"})),
            )
        })?;

    upload_to_gcs(&url, &file).await.map_err(|err| {
        eprintln!("Error uploading to GCS: {:?}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": "Failed to upload file to storage"})),
        )
    })?;

    let db = state.database.clone();
    let user = auth.username.clone();
    let fname = file.filename.clone();
    let fbytes = file.data.clone();

    tokio::spawn(async move {
        if let Err(e) = crate::worker::process_and_embed(db, user, fname, fbytes).await {
            eprintln!("Failed background embedding: {:?}", e);
        }
    });



    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "message": "File uploaded successfully",
            "filename": file.filename,
            "size": file.data.len()
        })),
    ))
}

pub async fn upload_to_gcs(signed_url: &str, file: &UploadedFile) -> anyhow::Result<()> {
    let response = reqwest::Client::new()
        .put(signed_url)
        .header("Content-Type", "text/markdown")
        .body(file.data.clone())
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("GCS upload failed: {}", response.status());
    }

    Ok(())
}