use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// 1 baris `strategies` — identitas & status strategi buatan user. Isi script ada di
/// `strategy_versions` (versi yang dipakai = `current_version_id`).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Strategy {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// `SCREENER` | `BOT`
    pub kind: String,
    /// `PRIVATE` | `PUBLIC`
    pub visibility: String,
    /// `DRAFT` | `ACTIVE` | `ARCHIVED`
    pub status: String,
    pub current_version_id: Option<Uuid>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
