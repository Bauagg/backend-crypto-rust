use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// 1 baris `store_purchases` — hak pakai strategi Store (pembelian / trial).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StorePurchase {
    pub id: Uuid,
    pub user_id: Uuid,
    pub listing_id: Uuid,
    pub strategy_id: Uuid,
    /// `PURCHASE` | `TRIAL`
    pub kind: String,
    pub pricing_model: String,
    pub list_price_idr: Decimal,
    pub discount_idr: Decimal,
    pub paid_idr: Decimal,
    pub promo_id: Option<Uuid>,
    /// `DUMMY` (saat ini, tanpa pembayaran asli) | `FREE` | `PAID`
    pub payment_status: String,
    pub payment_reference: Option<String>,
    /// `ACTIVE` | `EXPIRED` | `CANCELLED`
    pub status: String,
    pub starts_at: DateTime<Utc>,
    /// `None` = berlaku selamanya.
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
