CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS document_chunks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT NOT NULL,
    filename TEXT NOT NULL,
    chunk_index INT NOT NULL,
    content TEXT NOT NULL,
    embedding vector(768)
);

CREATE INDEX IF NOT EXISTS idx_chunks_user ON document_chunks (username);
CREATE INDEX IF NOT EXISTS idx_chunks_user_file ON document_chunks (username, filename);
-- ponytail: ivfflat/hnsw index deferred until document count > 10,000

SELECT * FROM document_chunks;

DELETE FROM document_chunks;