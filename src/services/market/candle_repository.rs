use rust_decimal::Decimal;
use sqlx::PgPool;

use super::candle_model::MarketCandle;
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

/// Ambil histori candle untuk 1 simbol+interval, terurut waktu — dipakai untuk export dataset ML.
pub async fn find_candles(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    limit: i64,
) -> Result<Vec<MarketCandle>, AppError> {
    let candles = sqlx::query_as::<_, MarketCandle>(
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

    Ok(candles)
}
