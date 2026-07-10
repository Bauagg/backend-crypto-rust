use std::str::FromStr;

use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;

use super::repository::insert_candle;
use crate::utils::app_error::AppError;
use crate::utils::http_client::get_json;

const BACKFILL_INTERVAL: &str = "1d";
/// ~2 tahun histori harian — cukup untuk `/score-live` (butuh minimal 60, idealnya >=200 bar)
/// dan untuk keperluan analisis/dataset ML lain ke depan.
const BACKFILL_LIMIT: u16 = 500;

fn api_base_url() -> String {
    std::env::var("MARKET_API_BASE_URL")
        .unwrap_or_else(|_| "https://www.tokocrypto.site".to_string())
}

/// Isi histori awal `market_candles` untuk satu simbol dari REST Tokocrypto (`/api/v3/klines`).
/// Idempotent lewat `insert_candle` (ON CONFLICT DO NOTHING) — aman dipanggil berkali-kali,
/// tidak akan menduplikasi baris yang sudah ada dari collector live.
pub async fn backfill_symbol(pool: &PgPool, symbol: &str) -> Result<usize, AppError> {
    let base_url = api_base_url();
    let url = format!(
        "{base_url}/api/v3/klines?symbol={symbol}&interval={BACKFILL_INTERVAL}&limit={BACKFILL_LIMIT}"
    );

    let raw: Vec<Value> = get_json(&url).await?;
    let mut inserted = 0;

    for row in raw {
        let Some(row) = row.as_array() else { continue };
        let (
            Some(open_time),
            Some(open),
            Some(high),
            Some(low),
            Some(close),
            Some(volume),
            Some(close_time),
        ) = (
            row.first().and_then(|v| v.as_i64()),
            row.get(1).and_then(|v| v.as_str()).and_then(|s| Decimal::from_str(s).ok()),
            row.get(2).and_then(|v| v.as_str()).and_then(|s| Decimal::from_str(s).ok()),
            row.get(3).and_then(|v| v.as_str()).and_then(|s| Decimal::from_str(s).ok()),
            row.get(4).and_then(|v| v.as_str()).and_then(|s| Decimal::from_str(s).ok()),
            row.get(5).and_then(|v| v.as_str()).and_then(|s| Decimal::from_str(s).ok()),
            row.get(6).and_then(|v| v.as_i64()),
        )
        else {
            continue;
        };

        insert_candle(
            pool,
            symbol,
            BACKFILL_INTERVAL,
            open_time,
            close_time,
            open,
            high,
            low,
            close,
            volume,
        )
        .await?;
        inserted += 1;
    }

    Ok(inserted)
}

/// Backfill histori untuk banyak simbol sekaligus (dipanggil sekali saat startup, sebelum
/// collector live mulai jalan). Kegagalan pada satu simbol tidak menghentikan simbol lain.
pub async fn backfill_symbols(pool: &PgPool, symbols: &[String]) {
    for symbol in symbols {
        match backfill_symbol(pool, symbol).await {
            Ok(count) => tracing::info!("Backfill {symbol}: {count} candle diproses"),
            Err(err) => tracing::error!("Backfill {symbol} gagal: {err:?}"),
        }
    }
}
