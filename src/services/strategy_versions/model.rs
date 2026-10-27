use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// 1 baris `strategy_versions` — isi script 1 versi strategi. Tidak pernah diubah setelah dibuat.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StrategyVersion {
    pub id: Uuid,
    pub strategy_id: Uuid,
    pub version_no: i32,
    /// `RHAI`
    pub language: String,
    pub source_code: String,
    /// Deklarasi setelan (`strategy_engine::inputs::InputSpec`).
    pub inputs: Value,
    /// `SCORE`
    pub output_type: String,
    /// `1D`
    pub timeframe: String,
    pub checksum: String,
    pub change_note: Option<String>,
    /// Robot BELI kalau skor >= ini.
    pub entry_score: Decimal,
    /// Robot JUAL kalau skor <= ini.
    pub exit_score: Decimal,
    /// Persen modal robot per coin kalau script tidak menentukan sendiri.
    pub position_percent: Decimal,
    pub max_positions: i16,
    /// `ALL` | `SPECIFIC`
    pub coin_scope: String,
    /// Coin untuk `SPECIFIC`, mis. `["BTCUSDT"]`.
    pub symbols: Vec<String>,
    // `ai_conversation` sengaja tidak dibaca ke struct ini: hanya disimpan sebagai bahan analisa/ML
    // (dibaca lewat SQL), tidak pernah dikirim ke FE.
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}
