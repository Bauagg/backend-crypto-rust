use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// 1 baris `store_listings` — strategi yang dipasang pembuatnya di Store.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StoreListing {
    pub id: Uuid,
    pub strategy_id: Uuid,
    pub seller_user_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    /// `FREE` | `ONE_TIME` | `MONTHLY`
    pub pricing_model: String,
    pub price_idr: Decimal,
    pub trial_days: i16,
    /// `ACTIVE` | `INACTIVE`
    pub status: String,
    pub published_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
