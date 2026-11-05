use crate::extractors::auth::AuthUser;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use pgvector::Vector;
use serde::Deserialize;
use serde_json::json;
use sqlx::{FromRow, PgPool};
use std::convert::Infallible;
use std::env;
use std::pin::Pin;

#[derive(Debug, FromRow)]
pub struct ChunkResult {
    pub filename: String,
    pub chunk_index: i32,
    pub content: String,
    pub similarity_score: f64,
}

#[derive(Deserialize, Debug)]
pub struct Query {
    query: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ask/query", post(ask))
        .route("/ask/stream", post(ask_stream))
}

const SYSTEM_PROMPT: &str = "You are a direct, concise assistant. Answer questions strictly using the provided context. If the answer cannot be found in the context, say 'I don't know based on the provided documents.' If the user asks a general/vague question and you have context about it, use it.";

// 1. Normal JSON Endpoint (POST /ask/query)
pub async fn ask(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<Query>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = reqwest::Client::new();

    let query_embedding = crate::worker::get_embedding(&client, &api_key, &payload.query)
        .await
        .map_err(|err| {
            eprintln!("Error while embedding query: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "Error", "message": "Internal Server Error"})),
            )
        })?;

    let similar_chunks = search_similar_chunks(&state.database, &auth.username, query_embedding, 5)
        .await
        .map_err(|err| {
            eprintln!("Error while retrieving relevant chunks: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "Error", "message": "Internal Server Error"})),
            )
        })?;

    let context = build_context_string(&similar_chunks);

    let answer = generate_rag_answer(&client, &api_key, SYSTEM_PROMPT, &context, &payload.query)
        .await
        .map_err(|err| {
            eprintln!("Error while generating answer: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "Error", "message": "Internal Server Error"})),
            )
        })?;

    let sources: Vec<_> = similar_chunks
        .into_iter()
        .map(|c| json!({ "filename": c.filename, "chunk_index": c.chunk_index, "score": c.similarity_score }))
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "Success",
            "answer": answer,
            "sources": sources
        })),
    ))
}

// 2. SSE Streaming Endpoint (POST /ask/stream)
pub async fn ask_stream(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<Query>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<serde_json::Value>)> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = reqwest::Client::new();

    let query_embedding = crate::worker::get_embedding(&client, &api_key, &payload.query)
        .await
        .map_err(|err| {
            eprintln!("Error while embedding query: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "Error", "message": "Internal Server Error"})),
            )
        })?;

    let similar_chunks = search_similar_chunks(&state.database, &auth.username, query_embedding, 5)
        .await
        .map_err(|err| {
            eprintln!("Error while retrieving relevant chunks: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "Error", "message": "Internal Server Error"})),
            )
        })?;

    let context = build_context_string(&similar_chunks);

    let stream = stream_gemini_rag_answer(&client, &api_key, SYSTEM_PROMPT, &context, &payload.query)
        .await
        .map_err(|err| {
            eprintln!("Error opening Gemini SSE stream: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "Error", "message": "Internal Server Error"})),
            )
        })?;

    let sse_stream = stream.map(|item| match item {
        Ok(delta) => Ok(Event::default().data(json!({ "delta": delta }).to_string())),
        Err(err) => Ok(Event::default().data(json!({ "error": err.to_string() }).to_string())),
    });

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()))
}

// Helper: build context with distinct document headers & newlines
fn build_context_string(chunks: &[ChunkResult]) -> String {
    let mut context = String::new();
    for chunk in chunks {
        context.push_str(&format!(
            "--- File: {} (Chunk {}, Score: {:.3}) ---\n{}\n\n",
            chunk.filename, chunk.chunk_index, chunk.similarity_score, chunk.content
        ));
    }
    context
}

pub async fn search_similar_chunks(
    db: &PgPool,
    username: &str,
    query_vector: Vec<f32>,
    limit: i64,
) -> Result<Vec<ChunkResult>, sqlx::Error> {
    let rows = sqlx::query_as::<_, ChunkResult>(
        r#"
        SELECT
            filename,
            chunk_index,
            content,
            (1.0 - (embedding <=> $1))::float8 AS similarity_score
        FROM document_chunks
        WHERE username = $2
        ORDER BY embedding <=> $1 ASC
        LIMIT $3
        "#,
    )
        .bind(Vector::from(query_vector))
        .bind(username)
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows)
}

pub async fn generate_rag_answer(
    client: &reqwest::Client,
    api_key: &str,
    system_prompt: &str,
    retrieved_chunks: &str,
    user_question: &str,
) -> anyhow::Result<String> {
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent";
    let user_prompt = format!("Context:\n{}\n\nQuestion:\n{}", retrieved_chunks, user_question);

    let payload = json!({
        "system_instruction": { "parts": [{ "text": system_prompt }] },
        "contents": [{ "role": "user", "parts": [{ "text": user_prompt }] }],
        "generationConfig": { "temperature": 0.2, "maxOutputTokens": 2048 }
    });

    let response = client
        .post(url)
        .header("x-goog-api-key", api_key)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let err = response.text().await?;
        anyhow::bail!("Gemini error: {}", err);
    }

    let json_resp: serde_json::Value = response.json().await?;
    let answer = json_resp["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing text in response: {:?}", json_resp))?
        .to_string();

    Ok(answer)
}

pub async fn stream_gemini_rag_answer(
    client: &reqwest::Client,
    api_key: &str,
    system_prompt: &str,
    retrieved_chunks: &str,
    user_question: &str,
) -> anyhow::Result<Pin<Box<dyn Stream<Item = Result<String, anyhow::Error>> + Send>>> {
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:streamGenerateContent?alt=sse";
    let user_prompt = format!("Context:\n{}\n\nQuestion:\n{}", retrieved_chunks, user_question);

    let payload = json!({
        "system_instruction": { "parts": [{ "text": system_prompt }] },
        "contents": [{ "role": "user", "parts": [{ "text": user_prompt }] }],
        "generationConfig": { "temperature": 0.2, "maxOutputTokens": 2048 }
    });

    let response = client
        .post(url)
        .header("x-goog-api-key", api_key)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let err = response.text().await?;
        anyhow::bail!("Gemini upstream error: {}", err);
    }

    let event_stream = response.bytes_stream().eventsource();

    let output_stream = event_stream.filter_map(|event_result| async move {
        match event_result {
            Ok(event) => {
                if event.data == "[DONE]" {
                    return None;
                }
                match serde_json::from_str::<serde_json::Value>(&event.data) {
                    Ok(json_chunk) => {
                        json_chunk["candidates"][0]["content"]["parts"][0]["text"]
                            .as_str()
                            .map(|text| Ok(text.to_string()))
                    }
                    Err(e) => Some(Err(anyhow::anyhow!("JSON parse error: {e}"))),
                }
            }
            Err(e) => Some(Err(anyhow::anyhow!("SSE stream error: {e}"))),
        }
    });

    Ok(Box::pin(output_stream))
}