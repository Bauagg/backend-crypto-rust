use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// 1 baris `user_screener_rules` — 1 indikator + kondisi di dalam strategi.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct UserScreenerRule {
    pub id: Uuid,
    pub screener_id: Uuid,
    pub flex_param_id: Uuid,
    pub indicator_code: String,
    pub params: Value,
    pub condition: String,
    pub weight: i16,
    pub sort_order: i16,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
