use std::net::SocketAddr;
use serverless_rag::state::AppState;
use dotenvy::dotenv;
use sqlx::postgres::PgPoolOptions;
use std::env;
use axum::serve;
use tokio::net::TcpListener;
use serverless_rag::routes;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let port_number = env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let db = PgPoolOptions::new()
        .max_connections(5)
        .idle_timeout(std::time::Duration::from_secs(300))
        .connect_lazy(&database_url)
        .expect("Database configuration error");

    let state = AppState { database: db };
    let app = routes::app_router().with_state(state);

    let listener_url = format!("0.0.0.0:{}", port_number);
    let listener = TcpListener::bind(listener_url).await.expect("error binding listener");

    // into_make_service_with_connect_info provides the SocketAddr needed by key extractors
    serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
        .await
        .expect("error starting server");
}