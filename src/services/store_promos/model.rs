use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// 1 baris `store_promos` — kode promo dari pembuat strategi untuk listing-nya.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StorePromo {
    pub id: Uuid,
    pub listing_id: Uuid,
    pub code: String,
    pub discount_percent: Decimal,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub max_redemptions: Option<i32>,
    pub redeemed_count: i32,
    pub is_active: bool,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StorePromo {
    /// Masih bisa dipakai sekarang: aktif, dalam periode, kuota belum habis.
    pub fn usable_now(&self) -> bool {
        let now = Utc::now();
        self.is_active
            && self.starts_at <= now
            && self.ends_at.is_none_or(|end| end > now)
            && self.max_redemptions.is_none_or(|max| self.redeemed_count < max)
    }
}
