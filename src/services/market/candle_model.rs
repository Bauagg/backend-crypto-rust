use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// Satu candle OHLCV yang sudah CLOSED (final, tidak pernah berubah lagi) — dipakai sebagai
/// dataset historis untuk analisis & training ML. Presisi harga pakai `Decimal`, bukan `f64`,
/// supaya tidak ada pembulatan mengambang yang bisa merusak akurasi data harga/volume.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct MarketCandle {
    pub id: Uuid,
    pub symbol: String,
    pub interval: String,
    pub open_time: i64,
    pub close_time: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub created_at: DateTime<Utc>,
}
