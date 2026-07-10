use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct FlexParam {
    pub id: Uuid,
    pub type_param: String,
    pub value_param: String,
    pub description: Option<String>,
    pub user_id: Uuid,
    pub photo_id: Option<Uuid>,
    pub photo_url: Option<String>,
    pub header_id: Option<Uuid>,
    /// Otomatis ditentukan dari ada/tidaknya foto: `true` kalau punya `photo_url`, `false` kalau tidak.
    pub is_active: bool,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
