use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct FileRecord {
    pub id: Uuid,
    pub file_name: String,
    pub file_url: String,
    pub file_type: String,
    pub user_id: Uuid,
    pub ref_id: Option<Uuid>,
    pub ref_type: Option<String>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
