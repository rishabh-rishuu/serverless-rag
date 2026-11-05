CREATE EXTENSION IF NOT EXISTS pgcrypto;
SELECT * FROM pg_available_extensions WHERE name = 'pgcrypto';

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT UNIQUE NOT NULL,
    password TEXT NOT NULL
);

DROP TABLE users;

CREATE TABLE tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT NOT NULL,
    token UUID UNIQUE DEFAULT gen_random_uuid(),
    expiring TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '1 days')
);

CREATE INDEX idx_tokens ON tokens (token);

SELECT * FROM tokens;

--2c4e4d7c-34dd-47b3-ab17-925b6f7a2556