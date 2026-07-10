use axum::{
    extract::{Query, State},
    response::Response,
};
use sqlx::PgPool;

use super::service::get_candles_service;
use super::types::StoredCandlesQuery;
use crate::utils::api_response::success;
use crate::utils::app_error::AppError;

const STORED_INTERVAL: &str = "1d";

/// Baca data historis dari `market_candles` (dikumpulkan sendiri oleh background collector +
/// backfill, bukan proxy live ke Tokocrypto) — cocok untuk export dataset ML, dan sebagai
/// fallback chart harian kalau proxy live (`/api/market/klines`) sedang tidak bisa diakses.
/// Hanya menyediakan interval 1d karena itu satu-satunya yang dikumpulkan.
pub async fn get_stored_candles(
    State(pool): State<PgPool>,
    Query(query): Query<StoredCandlesQuery>,
) -> Result<Response, AppError> {
    if query.symbol.trim().is_empty() {
        return Err(AppError::BadRequest("symbol wajib diisi".to_string()));
    }

    let limit = query.limit.unwrap_or(500).clamp(1, 5000) as i64;
    let symbol = query.symbol.trim().to_uppercase();

    let candles =
        get_candles_service(&pool, &symbol, STORED_INTERVAL, limit, query.end_time).await?;
    Ok(success(candles, "Berhasil mengambil data candle tersimpan"))
}
