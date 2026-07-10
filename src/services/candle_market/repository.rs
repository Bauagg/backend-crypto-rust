use rust_decimal::Decimal;
use sqlx::PgPool;

use super::model::MarketCandle;
use crate::utils::app_error::AppError;

/// Insert candle final ke `market_candles`. Idempotent: kalau kombinasi symbol+interval+open_time
/// sudah ada (mis. reconnect ulang mengirim event yang sama lagi), baris lama dipertahankan
/// apa adanya — data closed tidak pernah berubah, jadi tidak perlu di-update.
#[allow(clippy::too_many_arguments)]
pub async fn insert_candle(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    open_time: i64,
    close_time: i64,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: Decimal,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO market_candles (symbol, interval, open_time, close_time, open, high, low, close, volume)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (symbol, interval, open_time) DO NOTHING
        "#,
    )
    .bind(symbol)
    .bind(interval)
    .bind(open_time)
    .bind(close_time)
    .bind(open)
    .bind(high)
    .bind(low)
    .bind(close)
    .bind(volume)
    .execute(pool)
    .await?;

    Ok(())
}

/// Ambil histori candle untuk 1 simbol+interval, terurut baru->lama — dipakai untuk export
/// dataset ML / ditampilkan ke FE. `end_time` opsional untuk geser-ke-kiri (pagination by waktu):
/// isi dengan `open_time` candle paling lama yang sudah dimiliki FE, dapat balik candle-candle
/// SEBELUM waktu itu — sama seperti `/klines`, supaya `/candles` bisa jadi fallback kalau proxy
/// live Tokocrypto sedang mati.
pub async fn find_candles(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    limit: i64,
    end_time: Option<i64>,
) -> Result<Vec<MarketCandle>, AppError> {
    let candles = sqlx::query_as::<_, MarketCandle>(
        r#"
        SELECT * FROM market_candles
        WHERE symbol = $1 AND interval = $2 AND ($3::BIGINT IS NULL OR open_time < $3)
        ORDER BY open_time DESC
        LIMIT $4
        "#,
    )
    .bind(symbol)
    .bind(interval)
    .bind(end_time)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(candles)
}

/// Ambil `limit` candle TERAKHIR untuk 1 simbol+interval, tapi dikembalikan terurut lama->baru
/// (ASC) — bentuk yang dibutuhkan saat mengirim histori ke API ML (`POST /score-live`).
pub async fn find_recent_candles_asc(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    limit: i64,
) -> Result<Vec<MarketCandle>, AppError> {
    let mut candles = sqlx::query_as::<_, MarketCandle>(
        r#"
        SELECT * FROM market_candles
        WHERE symbol = $1 AND interval = $2
        ORDER BY open_time DESC
        LIMIT $3
        "#,
    )
    .bind(symbol)
    .bind(interval)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    candles.reverse();
    Ok(candles)
}
