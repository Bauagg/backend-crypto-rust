use axum::{
    extract::{Multipart, State},
    response::Response,
    Extension, Json,
};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use std::str::FromStr;

use super::service::{
    forgot_password_service, get_profile_service, login_service, register_service,
    reset_password_service, update_profile_service, verify_otp_service, ProfilePhotoUpload,
    FORGOT_PASSWORD_MESSAGE,
};
use super::types::{
    ForgotPasswordInput, LoginInput, RefreshTokenInput, RegisterInput, ResetPasswordInput,
    UpdateProfileInput, VerifyOtpInput,
};
use crate::utils::api_response::{created, success};
use crate::utils::app_error::AppError;
use crate::utils::jwt::{refresh_access_token, JwtClaims};

pub async fn register(
    State(pool): State<PgPool>,
    Json(body): Json<RegisterInput>,
) -> Result<Response, AppError> {
    let result = register_service(&pool, body).await?;
    Ok(created(result, "Registrasi berhasil"))
}

pub async fn login(
    State(pool): State<PgPool>,
    Json(body): Json<LoginInput>,
) -> Result<Response, AppError> {
    let result = login_service(&pool, body).await?;
    Ok(success(result, "Login berhasil"))
}

pub async fn get_profile(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let user_id = claims.user_uuid()?;
    let result = get_profile_service(&pool, user_id).await?;
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
            "demo_robot_capital_percent" => {
                input.demo_robot_capital_percent =
                    Some(parse_percent_field(&name, field.text().await.unwrap_or_default())?);
            }
            "platform_robot_capital_percent" => {
                input.platform_robot_capital_percent =
                    Some(parse_percent_field(&name, field.text().await.unwrap_or_default())?);
            }
            _ => {}
        }
    }

    Ok((input, photo))
}

/// Angka persen dari form (`"10"`, `"12.5"`); bukan angka = ditolak, tidak diam-diam diabaikan.
fn parse_percent_field(name: &str, value: String) -> Result<Decimal, AppError> {
    Decimal::from_str(value.trim())
        .map_err(|_| AppError::BadRequest(format!("{name} harus berupa angka 1–100")))
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
    let user_id = claims.user_uuid()?;
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

/// Lupa password langkah 1 — kirim kode OTP ke email. Body: `{ "email" }`.
/// Selalu dijawab pesan yang sama, terdaftar atau tidak.
pub async fn forgot_password(
    State(pool): State<PgPool>,
    Json(body): Json<ForgotPasswordInput>,
) -> Result<Response, AppError> {
    forgot_password_service(&pool, body).await?;
    Ok(success(
        json!({ "otp_valid_minutes": 10, "resend_after_seconds": 60 }),
        FORGOT_PASSWORD_MESSAGE,
    ))
}

/// Lupa password langkah 2 — cocokkan kode OTP. Body: `{ "email", "otp" }`.
pub async fn verify_otp(
    State(pool): State<PgPool>,
    Json(body): Json<VerifyOtpInput>,
) -> Result<Response, AppError> {
    let result = verify_otp_service(&pool, body).await?;
    Ok(success(result, "Kode OTP benar, silakan buat password baru"))
}

/// Lupa password langkah 3 — simpan password baru. Body: `{ "reset_token", "new_password", "confirm_password" }`.
pub async fn reset_password(
    State(pool): State<PgPool>,
    Json(body): Json<ResetPasswordInput>,
) -> Result<Response, AppError> {
    reset_password_service(&pool, body).await?;
    Ok(success(json!(null), "Password berhasil diubah, silakan login dengan password baru"))
}
