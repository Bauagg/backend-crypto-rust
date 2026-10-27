use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::model::StrategyVersion;

/// Isi versi baru dari user (body create strategi / versi baru).
#[derive(Debug, Clone, Deserialize)]
pub struct VersionInput {
    pub source_code: String,
    /// Deklarasi setelan, mis. `[{"name": "rsi_period", "label": "Periode RSI", "default": 14,
    /// "min": 2, "max": 100, "integer": true}]`. Kosong = tanpa setelan.
    #[serde(default)]
    pub inputs: Value,
    #[serde(default)]
    pub change_note: Option<String>,
    /// Riwayat chat asisten AI yang menghasilkan script ini (diisi FE kalau script dari ai-chat).
    #[serde(default)]
    pub ai_conversation: Option<Value>,
    /// Aturan robot & cakupan coin — kosong = sama dengan versi sebelumnya (atau default untuk
    /// versi pertama: masuk 70, keluar 30, 20% per coin, maks 5 posisi, semua coin).
    #[serde(flatten)]
    pub bot: BotConfigInput,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BotConfigInput {
    /// Robot BELI kalau skor >= ini (0–100).
    #[serde(default)]
    pub entry_score: Option<Decimal>,
    /// Robot JUAL kalau skor <= ini (0–100, lebih kecil dari entry_score).
    #[serde(default)]
    pub exit_score: Option<Decimal>,
    /// Persen modal robot per coin kalau script tidak menentukan sendiri (1–100).
    #[serde(default)]
    pub position_percent: Option<Decimal>,
    /// Maksimal coin dipegang bersamaan (1–50).
    #[serde(default)]
    pub max_positions: Option<i16>,
    /// `ALL` (semua coin aktif, bisa juga jadi screener) | `SPECIFIC` (hanya `symbols`, khusus robot).
    #[serde(default)]
    pub coin_scope: Option<String>,
    /// Coin untuk `SPECIFIC`, mis. `["BTC", "ETHUSDT"]` (maks 20).
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
}

/// Aturan robot yang sudah divalidasi.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BotConfig {
    pub entry_score: Decimal,
    pub exit_score: Decimal,
    pub position_percent: Decimal,
    pub max_positions: i16,
    pub coin_scope: String,
    pub symbols: Vec<String>,
}

impl BotConfig {
    pub fn of(version: &StrategyVersion) -> Self {
        Self {
            entry_score: version.entry_score,
            exit_score: version.exit_score,
            position_percent: version.position_percent,
            max_positions: version.max_positions,
            coin_scope: version.coin_scope.clone(),
            symbols: version.symbols.clone(),
        }
    }

    /// `true` = semua coin aktif (boleh dipakai sebagai screener).
    pub fn all_coins(&self) -> bool {
        self.coin_scope == "ALL"
    }

    /// Filter coin untuk menjalankan script: `None` = semua coin aktif.
    pub fn scope(&self) -> Option<&[String]> {
        (!self.all_coins()).then_some(self.symbols.as_slice())
    }
}

/// Versi untuk ditampilkan. `source_code` hanya untuk pemilik strategi — pemakai strategi orang
/// lain tidak boleh melihat isi script-nya.
#[derive(Debug, Serialize)]
pub struct VersionView {
    pub id: Uuid,
    pub version_no: i32,
    pub language: String,
    pub source_code: Option<String>,
    pub inputs: Value,
    pub output_type: String,
    pub timeframe: String,
    pub checksum: String,
    pub change_note: Option<String>,
    #[serde(flatten)]
    pub bot: BotConfig,
    pub created_at: DateTime<Utc>,
}

impl VersionView {
    pub fn new(version: StrategyVersion, show_source: bool) -> Self {
        let bot = BotConfig::of(&version);
        Self {
            bot,
            id: version.id,
            version_no: version.version_no,
            language: version.language,
            source_code: show_source.then_some(version.source_code),
            inputs: version.inputs,
            output_type: version.output_type,
            timeframe: version.timeframe,
            checksum: version.checksum,
            change_note: version.change_note,
            created_at: version.created_at,
        }
    }
}
