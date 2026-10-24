use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// 1 baris `user_screeners` — strategi screening milik user.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct UserScreener {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub max_results: i16,
    pub min_score: i16,
    pub is_active: bool,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
