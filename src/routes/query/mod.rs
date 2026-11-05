use axum::Router;
use crate::state::AppState;

pub mod ask;

pub fn router() -> Router<AppState> {
    Router::new().merge(ask::router())
}