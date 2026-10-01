use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

/// Satu baris tabel `fear_greed_index` — nilai Fear & Greed Index (sentimen pasar crypto, 0-100)
/// untuk satu tanggal, di-cache dari alternative.me. Nilai historis tidak pernah berubah.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct FearGreedIndex {
    pub date: NaiveDate,
    pub fng_value: i16,
    pub created_at: DateTime<Utc>,
}
