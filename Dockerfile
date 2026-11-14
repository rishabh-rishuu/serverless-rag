FROM rust:1-slim-bookworm as builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && touch src/lib.rs && echo "fn main() {}" > src/main.rs
RUN cargo build --release -j $(nproc)

RUN rm -rf src target/release/deps/serverless_rag* target/release/deps/libserverless_rag* target/release/serverless_rag

COPY src ./src
RUN cargo build --release -j $(nproc)

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/serverless_rag /usr/local/bin/server
ENV PORT=8080
EXPOSE 8080
CMD ["server"]
