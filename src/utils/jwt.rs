use std::sync::OnceLock;

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

/// Kunci & masa berlaku token — dibaca dari env sekali saja (`init`), bukan tiap request.
struct JwtKeys {
    access_encoding: EncodingKey,
    access_decoding: DecodingKey,
    refresh_encoding: EncodingKey,
    refresh_decoding: DecodingKey,
    access_ttl: i64,
    refresh_ttl: i64,
}

static KEYS: OnceLock<JwtKeys> = OnceLock::new();

fn load_keys() -> Result<JwtKeys, AppError> {
    let env = |key: &str| {
        std::env::var(key)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| AppError::Internal(format!("{key} tidak ditemukan di .env")))
    };
    let secret = env("JWT_SECRET")?;
    let refresh_secret = env("JWT_REFRESH_SECRET")?;
    Ok(JwtKeys {
        access_encoding: EncodingKey::from_secret(secret.as_bytes()),
        access_decoding: DecodingKey::from_secret(secret.as_bytes()),
        refresh_encoding: EncodingKey::from_secret(refresh_secret.as_bytes()),
        refresh_decoding: DecodingKey::from_secret(refresh_secret.as_bytes()),
        access_ttl: expires_in_seconds("JWT_EXPIRES_IN", "15m"),
        refresh_ttl: expires_in_seconds("JWT_REFRESH_EXPIRES_IN", "7d"),
    })
}

/// Baca kunci JWT saat server start — env kosong = server gagal start, bukan error saat request.
pub fn init() -> Result<(), AppError> {
    keys().map(|_| ())
}

fn keys() -> Result<&'static JwtKeys, AppError> {
    if let Some(keys) = KEYS.get() {
        return Ok(keys);
    }
    let keys = load_keys()?;
    Ok(KEYS.get_or_init(|| keys))
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
    let keys = keys()?;
    let claims = build_claims(payload, keys.access_ttl);
    encode(&Header::default(), &claims, &keys.access_encoding)
        .map_err(|_| AppError::Internal("Gagal membuat token".to_string()))
}

pub fn verify_token(token: &str) -> Result<JwtClaims, AppError> {
    decode::<JwtClaims>(token, &keys()?.access_decoding, &Validation::default())
        .map(|data| data.claims)
        .map_err(|_| AppError::Unauthorized("Token tidak valid".to_string()))
}

pub fn sign_refresh_token(payload: &JwtPayload) -> Result<String, AppError> {
    let keys = keys()?;
    let claims = build_claims(payload, keys.refresh_ttl);
    encode(&Header::default(), &claims, &keys.refresh_encoding)
        .map_err(|_| AppError::Internal("Gagal membuat refresh token".to_string()))
}

pub fn verify_refresh_token(token: &str) -> Result<JwtClaims, AppError> {
    decode::<JwtClaims>(token, &keys()?.refresh_decoding, &Validation::default())
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
