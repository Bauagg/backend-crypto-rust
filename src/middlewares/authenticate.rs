use axum::{
    extract::Request,
    http::header,
    middleware::Next,
    response::Response,
};

use crate::utils::app_error::AppError;
use crate::utils::jwt::verify_token;

/// Verifikasi header `Authorization: Bearer <token>`, lalu inject `JwtClaims`
/// ke request extensions supaya bisa diambil handler lewat `Extension<JwtClaims>`.
pub async fn authenticate(mut request: Request, next: Next) -> Result<Response, AppError> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = match auth_header.and_then(|h| h.strip_prefix("Bearer ")) {
        Some(token) => token,
        None => {
            return Err(AppError::Unauthorized(
                "Header Authorization dengan Bearer token wajib disertakan".to_string(),
            ))
        }
    };

    let claims = verify_token(token)?;
    request.extensions_mut().insert(claims);

    Ok(next.run(request).await)
}
