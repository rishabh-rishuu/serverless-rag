# serverless-rag

A lightweight RAG backend built in Rust to index my markdown vault and query it from anywhere. 

Files get uploaded to Google Cloud Storage, chunked and embedded in the background using Gemini (`gemini-embedding-001`), and stored in Postgres with `pgvector`. It runs on Cloud Run so it scales to zero when I'm not using it.

## Stack

- **Rust** (`axum`, `tokio`, `sqlx`, `reqwest`)
- **Postgres** + `pgvector` (768-dim cosine distance)
- **Google Cloud Storage** (raw `.md` and `.txt` storage)
- **Gemini API** (`gemini-embedding-001` for embeddings, `gemini-1.5-flash` for answers)
- **Auth:** Argon2id password hashes + UUID bearer tokens + `tower-governor` rate limiting

## How it works

1. Hit `/gcp/upload-file` with a `.md` or `.txt` file.
2. The server generates a signed URL, pushes the raw file to GCS, and spawns a background task.
3. The worker splits the text along safe UTF-8 boundaries (1000 chars, 200 overlap), grabs embeddings in batches of 100, and runs a Postgres transaction to wipe old chunks for that file before inserting the new ones.
4. Hitting `/ask/query` or `/ask/stream` embeds the question, grabs the top 5 closest chunks from Postgres using `<=>`, and sends them to Gemini.

## Database schema

Requires a Postgres instance with `vector` and `uuid-ossp`:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username TEXT UNIQUE NOT NULL,
    password TEXT NOT NULL
);

CREATE TABLE tokens (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username TEXT NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    token UUID UNIQUE DEFAULT uuid_generate_v4(),
    expiring TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '30 days')
);

CREATE TABLE document_chunks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username TEXT NOT NULL,
    filename TEXT NOT NULL,
    chunk_index INT NOT NULL,
    content TEXT NOT NULL,
    embedding VECTOR(768) NOT NULL
);

CREATE INDEX idx_chunks_user_file ON document_chunks (username, filename);
CREATE INDEX idx_chunks_embedding_hnsw ON document_chunks USING hnsw (embedding vector_cosine_ops);
```

## Setup

Create a `.env` file:

```env
PORT=8080
DATABASE_URL=postgresql://postgres:password@localhost:5432/serverless-rag
GEMINI_API_KEY=your_key
GCS_BUCKET_NAME=serverless-rag
GCP_PROJECT_ID=your_project_id
```

Run locally:

```bash
cargo run --release
```

## Endpoints

### Auth
- `POST /auth/register` — `{"username": "...", "password": "..."}`
- `POST /auth/login` — returns `{"token": "uuid..."}`

### Upload
- `POST /gcp/upload-file` — Multipart form upload (`file`). Max 20MB, `.md` and `.txt` only. Re-uploading a file with the same name overwrites the GCS object and replaces its chunks in the DB.

### Query
- `POST /ask/query` — Standard JSON response with answer + matched source chunks.
- `POST /ask/stream` — Server-Sent Events (`text/event-stream`) token stream.

Example query:

```bash
curl -X POST http://localhost:8080/ask/query \
  -H "Authorization: Bearer <TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{"query": "What are the notes on agents?"}'
```

Streaming:

```bash
curl -N -X POST http://localhost:8080/ask/stream \
  -H "Authorization: Bearer <TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{"query": "Summarize the agent guidelines."}'
```

## Deploying to Cloud Run

Uses Cloud Build with Kaniko layer caching (`cloudbuild.yaml`) so Cargo dependencies don't recompile on every push:

```bash
gcloud builds submit . --region=asia-south1 --config=cloudbuild.yaml

gcloud run deploy rag-service \
  --image=asia-south1-docker.pkg.dev/<PROJECT_ID>/cloud-run-source-deploy/rag-service:latest \
  --region=asia-south1 \
  --execution-environment=gen2 \
  --cpu=1 \
  --memory=512Mi \
  --no-cpu-throttling \
  --allow-unauthenticated
```
