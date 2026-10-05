use axum::{
    extract::{Multipart, State},
    response::Response,
    Extension, Json,
};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use std::str::FromStr;
use uuid::Uuid;

use super::service::{
    get_profile_service, login_service, register_service, update_profile_service,
    ProfilePhotoUpload,
};
use super::types::{LoginInput, RefreshTokenInput, RegisterInput, UpdateProfileInput};
use crate::utils::api_response::{created, success};
use crate::utils::app_error::AppError;
use crate::utils::jwt::{refresh_access_token, JwtClaims};

fn parse_user_id(claims: &JwtClaims) -> Result<Uuid, AppError> {
    Uuid::parse_str(&claims.user_id).map_err(|_| AppError::Unauthorized("Token tidak valid".to_string()))
}

async fn begin_tx(pool: &PgPool) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, AppError> {
    pool.begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))
}

async fn commit_tx(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), AppError> {
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))
}

pub async fn register(
    State(pool): State<PgPool>,
    Json(body): Json<RegisterInput>,
) -> Result<Response, AppError> {
    let mut tx = begin_tx(&pool).await?;

    let result = match register_service(&mut tx, body).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(created(result, "Registrasi berhasil"))
}

pub async fn login(
    State(pool): State<PgPool>,
    Json(body): Json<LoginInput>,
) -> Result<Response, AppError> {
    let mut tx = begin_tx(&pool).await?;

    let result = match login_service(&mut tx, body).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(success(result, "Login berhasil"))
}

pub async fn get_profile(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let user_id = parse_user_id(&claims)?;
    let mut tx = begin_tx(&pool).await?;

    let result = match get_profile_service(&mut tx, user_id).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(success(result, "Berhasil mengambil profil"))
}

/// Terima multipart form-data: field teks (full_name, email, phone, platform, api_key, api_secret)
/// semuanya opsional, plus field file "photo" opsional untuk ganti foto profil.
async fn parse_update_profile_multipart(
    mut multipart: Multipart,
) -> Result<(UpdateProfileInput, Option<ProfilePhotoUpload>), AppError> {
    let mut input = UpdateProfileInput::default();
    let mut photo: Option<ProfilePhotoUpload> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("Form data tidak valid".to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "photo" => {
                let original_name = field.file_name().unwrap_or("photo").to_string();
                let mime_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Gagal membaca foto".to_string()))?;
                photo = Some(ProfilePhotoUpload {
                    original_name,
                    mime_type,
                    bytes: bytes.to_vec(),
                });
            }
            "full_name" => input.full_name = non_empty(field.text().await.unwrap_or_default()),
            "email" => input.email = non_empty(field.text().await.unwrap_or_default()),
            "phone" => input.phone = non_empty(field.text().await.unwrap_or_default()),
            "platform" => input.platform = non_empty(field.text().await.unwrap_or_default()),
            "api_key" => input.api_key = non_empty(field.text().await.unwrap_or_default()),
            "api_secret" => input.api_secret = non_empty(field.text().await.unwrap_or_default()),
            "preferred_currency" => {
                input.preferred_currency = non_empty(field.text().await.unwrap_or_default())
            }
            "demo_balance" => {
                let value = field.text().await.unwrap_or_default();
                input.demo_balance = Decimal::from_str(&value).ok();
            }
            "is_robot_demo_active" => {
                input.is_robot_demo_active = parse_bool_field(field.text().await.unwrap_or_default());
            }
            "is_robot_platform_active" => {
                input.is_robot_platform_active = parse_bool_field(field.text().await.unwrap_or_default());
            }
            _ => {}
        }
    }

    Ok((input, photo))
}

fn parse_bool_field(value: String) -> Option<bool> {
    match value.as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub async fn update_profile(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let user_id = parse_user_id(&claims)?;
    let (input, photo) = parse_update_profile_multipart(multipart).await?;

    // Transaksi diatur di service: verifikasi ke Binance dulu (tanpa tx), baru tulis ke DB.
    let result = update_profile_service(&pool, user_id, input, photo).await?;
    Ok(success(result, "Profil berhasil diupdate"))
}

pub async fn refresh_token(Json(body): Json<RefreshTokenInput>) -> Result<Response, AppError> {
    let access_token = refresh_access_token(&body.refresh_token)?;
    Ok(success(
        json!({ "access_token": access_token }),
        "Access token berhasil diperbarui",
    ))
}
