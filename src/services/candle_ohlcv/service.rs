use sqlx::PgPool;

use super::model::CandleOhlcv;
use super::repository::{find_candles, find_recent_candles_asc};
use crate::utils::app_error::AppError;

/// Ambil histori candle untuk 1 simbol+interval, terurut baru->lama — dipakai untuk export
/// dataset ML / ditampilkan apa adanya ke FE. `end_time` opsional untuk geser-ke-kiri (load
/// histori lebih lama), sama seperti `/klines`.
pub async fn get_candles_service(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    limit: i64,
    end_time: Option<i64>,
) -> Result<Vec<CandleOhlcv>, AppError> {
    find_candles(pool, symbol, interval, limit, end_time).await
}

/// Ambil `limit` candle terakhir untuk 1 simbol+interval, terurut lama->baru — dipakai
/// `market::service` untuk fallback harga kalau ticker exchange gagal.
pub async fn get_recent_candles_asc_service(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    limit: i64,
) -> Result<Vec<CandleOhlcv>, AppError> {
    find_recent_candles_asc(pool, symbol, interval, limit).await
}
