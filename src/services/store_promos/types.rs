use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

/// Body `POST /api/store/promos`.
#[derive(Debug, Deserialize)]
pub struct CreatePromoInput {
    pub listing_id: Uuid,
    /// 3–30 karakter huruf/angka/`-`/`_`, disimpan huruf besar.
    pub code: String,
    /// Potongan harga 1–100 %.
    pub discount_percent: Decimal,
    /// Default sekarang.
    #[serde(default)]
    pub starts_at: Option<DateTime<Utc>>,
    /// Kosong = tanpa batas akhir.
    #[serde(default)]
    pub ends_at: Option<DateTime<Utc>>,
    /// Kosong = tanpa batas jumlah pemakaian.
    #[serde(default)]
    pub max_redemptions: Option<i32>,
}

/// Body `PUT /api/store/promos/:id` — field yang tidak dikirim tetap.
#[derive(Debug, Deserialize)]
pub struct UpdatePromoInput {
    pub code: Option<String>,
    pub discount_percent: Option<Decimal>,
    pub starts_at: Option<DateTime<Utc>>,
    /// `null` tidak bisa dibedakan dari "tidak dikirim" — pakai `clear_ends_at: true` untuk menghapus batas.
    pub ends_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub clear_ends_at: bool,
    pub max_redemptions: Option<i32>,
    #[serde(default)]
    pub clear_max_redemptions: bool,
    pub is_active: Option<bool>,
}

/// Query `GET /api/store/promos?listing_id=`.
#[derive(Debug, Deserialize)]
pub struct PromoQuery {
    pub listing_id: Uuid,
}
