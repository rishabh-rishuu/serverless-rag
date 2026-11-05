pub mod auth;
mod gcp;
pub mod query;

use axum::Router;
use crate::state::AppState;
pub fn app_router() -> Router<AppState> {
    Router::new().merge(auth::router()).merge(gcp::router()).merge(query::router())
}