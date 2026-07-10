use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::utils::app_error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub username: String,
    pub exp: usize,
}

#[derive(Debug, Clone)]
pub struct JwtPayload {
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

fn secret() -> String {
    std::env::var("JWT_SECRET").expect("JWT_SECRET tidak ditemukan di .env")
}

fn refresh_secret() -> String {
    std::env::var("JWT_REFRESH_SECRET").expect("JWT_REFRESH_SECRET tidak ditemukan di .env")
}

fn expires_in_seconds(env_key: &str, default: &str) -> i64 {
    let raw = std::env::var(env_key).unwrap_or_else(|_| default.to_string());
    parse_duration_to_seconds(&raw)
}

fn parse_duration_to_seconds(raw: &str) -> i64 {
    let raw = raw.trim();
    let (num_part, unit) = raw.split_at(raw.len() - 1);
    match num_part.parse::<i64>() {
        Ok(n) => match unit {
            "s" => n,
            "m" => n * 60,
            "h" => n * 3600,
            "d" => n * 86400,
            _ => raw.parse::<i64>().unwrap_or(900),
        },
        Err(_) => raw.parse::<i64>().unwrap_or(900),
    }
}

fn build_claims(payload: &JwtPayload, ttl_seconds: i64) -> JwtClaims {
    let exp = (chrono::Utc::now().timestamp() + ttl_seconds) as usize;
    JwtClaims {
        user_id: payload.user_id.clone(),
        email: payload.email.clone(),
        role: payload.role.clone(),
        username: payload.username.clone(),
        exp,
    }
}

pub fn sign_token(payload: &JwtPayload) -> Result<String, AppError> {
    let ttl = expires_in_seconds("JWT_EXPIRES_IN", "15m");
    let claims = build_claims(payload, ttl);
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret().as_bytes()),
    )
    .map_err(|_| AppError::Internal("Gagal membuat token".to_string()))
}

pub fn verify_token(token: &str) -> Result<JwtClaims, AppError> {
    decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(secret().as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized("Token tidak valid".to_string()))
}

pub fn sign_refresh_token(payload: &JwtPayload) -> Result<String, AppError> {
    let ttl = expires_in_seconds("JWT_REFRESH_EXPIRES_IN", "7d");
    let claims = build_claims(payload, ttl);
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(refresh_secret().as_bytes()),
    )
    .map_err(|_| AppError::Internal("Gagal membuat refresh token".to_string()))
}

pub fn verify_refresh_token(token: &str) -> Result<JwtClaims, AppError> {
    decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(refresh_secret().as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized("Refresh token tidak valid".to_string()))
}

pub fn generate_token_pair(payload: &JwtPayload) -> Result<TokenPair, AppError> {
    Ok(TokenPair {
        access_token: sign_token(payload)?,
        refresh_token: sign_refresh_token(payload)?,
    })
}

pub fn refresh_access_token(refresh_token: &str) -> Result<String, AppError> {
    let claims = verify_refresh_token(refresh_token)?;
    sign_token(&JwtPayload {
        user_id: claims.user_id,
        email: claims.email,
        role: claims.role,
        username: claims.username,
    })
}
