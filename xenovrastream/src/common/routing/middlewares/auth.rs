use std::sync::Arc;

use axum::{
    extract::State,
    headers::{authorization::Bearer, Authorization, HeaderMapExt},
    http::{HeaderMap, HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use reqwest::StatusCode;

use crate::{
    common::{
        jwt_manager::{AuthUser, JWTManager},
        routing::app_state::AppState,
    },
    errors::{XenovraStreamError, XenovraStreamResult},
};

/// Middleware that requires to be loggen in
pub async fn logged_in_required<B>(
    State(state): State<Arc<AppState>>,
    mut req: Request<B>,
    next: Next<B>,
) -> Result<Response, (StatusCode, String)> {
    let auth_user = authenticate(req.headers(), &state.config.secret_key)
        .map_err(<(StatusCode, String)>::from)?;

    req.extensions_mut().insert(auth_user);
    Ok(next.run(req).await)
}

#[inline]
fn authenticate(headers: &HeaderMap<HeaderValue>, secret_key: &str) -> XenovraStreamResult<AuthUser> {
    let auth_header = headers
        .typed_get::<Authorization<Bearer>>()
        .ok_or(XenovraStreamError::NotAuthenticated)?;

    JWTManager::validate(auth_header.token(), secret_key)
}
