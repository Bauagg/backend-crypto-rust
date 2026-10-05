use std::collections::HashMap;

use sqlx::PgPool;

use super::model::CandleOhlcv;
use super::types::NewCandle;
use crate::utils::app_error::AppError;

/// Insert banyak candle final sekaligus dalam 1 query (bukan 1 query per candle — backfill 500
/// candle cukup 1 round-trip ke DB). Idempotent: kombinasi symbol+interval+open_time yang sudah
/// ada dipertahankan apa adanya — data closed tidak pernah berubah. Balik jumlah baris yang
/// benar-benar baru masuk.
pub async fn insert_candles(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    candles: &[NewCandle],
) -> Result<u64, AppError> {
    if candles.is_empty() {
        return Ok(0);
    }

    let result = sqlx::query(
        r#"
        INSERT INTO candle_ohlcv (symbol, interval, open_time, close_time, open, high, low, close, volume)
        SELECT $1, $2, * FROM UNNEST($3::BIGINT[], $4::BIGINT[], $5::NUMERIC[], $6::NUMERIC[],
                                     $7::NUMERIC[], $8::NUMERIC[], $9::NUMERIC[])
        ON CONFLICT (symbol, interval, open_time) DO NOTHING
        "#,
    )
    .bind(symbol)
    .bind(interval)
    .bind(candles.iter().map(|c| c.open_time).collect::<Vec<_>>())
    .bind(candles.iter().map(|c| c.close_time).collect::<Vec<_>>())
    .bind(candles.iter().map(|c| c.open).collect::<Vec<_>>())
    .bind(candles.iter().map(|c| c.high).collect::<Vec<_>>())
    .bind(candles.iter().map(|c| c.low).collect::<Vec<_>>())
    .bind(candles.iter().map(|c| c.close).collect::<Vec<_>>())
    .bind(candles.iter().map(|c| c.volume).collect::<Vec<_>>())
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
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
) -> Result<Vec<CandleOhlcv>, AppError> {
    let candles = sqlx::query_as::<_, CandleOhlcv>(
        r#"
        SELECT * FROM candle_ohlcv
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

/// `open_time` candle terbaru per simbol untuk 1 interval, semua simbol dalam 1 query — dipakai
/// `worker` untuk melewati simbol yang sudah lengkap dan fetch lanjutan dari candle berikutnya.
/// Simbol yang belum punya data tidak ada di map.
pub async fn find_latest_open_times(
    pool: &PgPool,
    interval: &str,
) -> Result<HashMap<String, i64>, AppError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT symbol, MAX(open_time) FROM candle_ohlcv WHERE interval = $1 GROUP BY symbol",
    )
    .bind(interval)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().collect())
}

/// Harga `close` 2 candle terakhir untuk BANYAK simbol dalam 1 query (bukan 1 query per simbol).
/// Balik `simbol -> (close terakhir, close sebelumnya kalau ada)`.
///
/// `LATERAL ... LIMIT 2` per simbol: index `(symbol, interval, open_time)` dibaca mundur dan
/// berhenti setelah 2 baris — tidak membaca & mengurutkan seluruh histori candle tiap simbol
/// (63 simbol: ±1 ms vs ±120 ms dengan window function di seluruh tabel).
pub async fn find_last_two_closes(
    pool: &PgPool,
    symbols: &[String],
    interval: &str,
) -> Result<HashMap<String, (rust_decimal::Decimal, Option<rust_decimal::Decimal>)>, AppError> {
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, rust_decimal::Decimal, i64)> = sqlx::query_as(
        r#"
        SELECT s.symbol, c.close, c.rn
        FROM unnest($2::text[]) AS s(symbol)
        CROSS JOIN LATERAL (
            SELECT close, ROW_NUMBER() OVER (ORDER BY open_time DESC) AS rn
            FROM candle_ohlcv
            WHERE symbol = s.symbol AND interval = $1
            ORDER BY open_time DESC
            LIMIT 2
        ) c
        "#,
    )
    .bind(interval)
    .bind(symbols)
    .fetch_all(pool)
    .await?;

    let mut closes: HashMap<String, (rust_decimal::Decimal, Option<rust_decimal::Decimal>)> =
        HashMap::new();
    for (symbol, close, rn) in rows {
        let entry = closes.entry(symbol).or_insert((close, None));
        if rn == 1 {
            entry.0 = close;
        } else {
            entry.1 = Some(close);
        }
    }
    Ok(closes)
}
