use pgvector::Vector;
use serde_json::json;
use sqlx::PgPool;
use std::env;

const CHUNK_SIZE: usize = 1000;
const CHUNK_OVERLAP: usize = 200;

pub async fn process_and_embed(
    db: PgPool,
    username: String,
    filename: String,
    raw_data: Vec<u8>,
) -> anyhow::Result<()> {
    let text = String::from_utf8(raw_data)?;
    let chunks = chunk_text(&text, CHUNK_SIZE, CHUNK_OVERLAP);
    if chunks.is_empty() {
        return Ok(());
    }

    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = reqwest::Client::new();

    // 1. Generate all embeddings in batches of 100
    let embeddings = get_embeddings_batch(&client, &api_key, &chunks).await?;

    // 2. Atomic DB wipe & replace
    let mut tx = db.begin().await?;

    // Delete existing chunks for this specific user & file
    sqlx::query("DELETE FROM document_chunks WHERE username = $1 AND filename = $2")
        .bind(&username)
        .bind(&filename)
        .execute(&mut *tx)
        .await?;

    // Insert new chunks
    for (index, (chunk, embedding)) in chunks.into_iter().zip(embeddings).enumerate() {
        sqlx::query(
            "INSERT INTO document_chunks (username, filename, chunk_index, content, embedding)
             VALUES ($1, $2, $3, $4, $5)",
        )
            .bind(&username)
            .bind(&filename)
            .bind(index as i32)
            .bind(chunk)
            .bind(Vector::from(embedding))
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

fn chunk_text(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let step = size.saturating_sub(overlap).max(1);

    while start < text.len() {
        let mut end = (start + size).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }

        chunks.push(text[start..end].to_string());
        if end == text.len() {
            break;
        }

        let mut next_start = (start + step).min(text.len());
        while !text.is_char_boundary(next_start) {
            next_start += 1;
        }
        start = next_start;
    }
    chunks
}

pub async fn get_embeddings_batch(
    client: &reqwest::Client,
    api_key: &str,
    chunks: &[String],
) -> anyhow::Result<Vec<Vec<f32>>> {
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:batchEmbedContents";
    let mut all_vectors = Vec::with_capacity(chunks.len());

    // Process in batches of 100 (API limit per batch call)
    for batch in chunks.chunks(100) {
        let requests: Vec<_> = batch
            .iter()
            .map(|text| {
                json!({
                    "model": "models/gemini-embedding-001",
                    "content": { "parts": [{ "text": text }] },
                    "outputDimensionality": 768
                })
            })
            .collect();

        let res = client
            .post(url)
            .header("x-goog-api-key", api_key)
            .json(&json!({ "requests": requests }))
            .send()
            .await?;

        if !res.status().is_success() {
            let error_text = res.text().await?;
            anyhow::bail!("Batch embedding failed: {}", error_text);
        }

        let json_res: serde_json::Value = res.json().await?;
        let embeddings = json_res["embeddings"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing batch embeddings: {:?}", json_res))?;

        for emb in embeddings {
            let vec: Vec<f32> = emb["values"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Missing values in embedding: {:?}", emb))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            all_vectors.push(vec);
        }
    }
    Ok(all_vectors)
}

// Single embedding helper for query search in ask.rs
pub async fn get_embedding(
    client: &reqwest::Client,
    api_key: &str,
    text: &str,
) -> anyhow::Result<Vec<f32>> {
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent";

    let res = client
        .post(url)
        .header("x-goog-api-key", api_key)
        .json(&json!({
            "model": "models/gemini-embedding-001",
            "content": { "parts": [{ "text": text }] },
            "outputDimensionality": 768
        }))
        .send()
        .await?;

    if !res.status().is_success() {
        let error_text = res.text().await?;
        anyhow::bail!("Single embedding failed: {}", error_text);
    }

    let json_res: serde_json::Value = res.json().await?;
    let values = json_res["embedding"]["values"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing embedding values: {:?}", json_res))?
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect::<Vec<f32>>();

    Ok(values)
}