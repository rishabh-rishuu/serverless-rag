pub mod createUrl;
pub mod upload_file;

use axum::Router;
use crate::routes::auth::login;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().merge(upload_file::router())
}