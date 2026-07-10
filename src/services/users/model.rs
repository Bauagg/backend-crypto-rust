use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
    pub phone: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub role: String,
    pub status: String,
    pub platform: Option<String>,
    pub api_key: Option<String>,
    /// Dipakai nanti oleh service bot trading (via `utils::crypto::decrypt`) saat eksekusi order ke exchange.
    #[allow(dead_code)]
    #[serde(skip_serializing)]
    pub api_secret: Option<String>,
    pub photo_id: Option<String>,
    pub photo_url: Option<String>,
    /// Saldo akun demo (representasi USD), default 1000, bisa diubah lewat endpoint profile.
    pub demo_balance: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
