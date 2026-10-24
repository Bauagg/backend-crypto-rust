use axum::{extract::State, response::Response, Extension, Json};
use sqlx::PgPool;

use super::service::{
    ai_chat_service, delete_screener_service, get_screener_service, get_screening_results_service,
    save_screener_service,
};
use super::types::{AiChatInput, ScreenerInput};
use crate::utils::api_response::success;
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// Strategi screening milik user yang login (`data: null` kalau belum punya).
/// Contoh: GET /api/screeners
pub async fn get_screener(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let screener = get_screener_service(&pool, claims.user_uuid()?).await?;
    Ok(success(screener, "Berhasil mengambil strategi screening"))
}

/// Simpan strategi (1 user = 1 strategi: dibuat kalau belum ada, kalau sudah ada diganti). Body
/// `{ "name", "description"?, "max_results"?, "min_score"?, "is_active"?, "rules": [{
/// "flex_param_id", "params"?, "condition", "weight"? }] }` — sama dengan `draft` dari ai-chat.
/// Contoh: PUT /api/screeners
pub async fn save_screener(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<ScreenerInput>,
) -> Result<Response, AppError> {
    let screener = save_screener_service(&pool, claims.user_uuid()?, &claims.email, input).await?;
    Ok(success(screener, "Strategi screening berhasil disimpan"))
}

/// Contoh: DELETE /api/screeners
pub async fn delete_screener(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    delete_screener_service(&pool, claims.user_uuid()?, &claims.email).await?;
    Ok(success(serde_json::Value::Null, "Strategi screening berhasil dihapus"))
}

/// Jalankan screening dengan strategi user: coin skor tertinggi (dihitung saat diminta).
/// Contoh: GET /api/screeners/results
pub async fn get_screening_results(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let result = get_screening_results_service(&pool, claims.user_uuid()?).await?;
    Ok(success(result, "Berhasil menjalankan screening"))
}

/// Chat dengan asisten AI untuk menyusun / mengubah strategi. Body `{ "messages": [{ "role":
/// "user" | "assistant", "content" }] }` (seluruh riwayat, pesan terakhir dari user). Balik `reply`,
/// dan `draft` (siap dikirim ke PUT /screeners) kalau AI sudah menyimpulkan strategi. Tidak
/// menyimpan apa pun. Contoh: POST /api/screeners/ai-chat
pub async fn ai_chat(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<AiChatInput>,
) -> Result<Response, AppError> {
    let result = ai_chat_service(&pool, claims.user_uuid()?, input).await?;
    Ok(success(result, "Berhasil mendapat balasan asisten AI"))
}
