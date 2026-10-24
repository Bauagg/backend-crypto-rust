use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// 1 indikator di dalam strategi. `params` & `condition` yang valid per indikator ditentukan
/// `spec()` di `screener_rules/service.rs`.
#[derive(Debug, Deserialize)]
pub struct ScreenerRuleInput {
    /// Indikator (flex_params `INDICATORS`).
    pub flex_param_id: Uuid,
    /// Setelan indikator, mis. `{"period": 13, "threshold": 30}`. Key yang tidak diisi = default.
    #[serde(default)]
    pub params: Value,
    pub condition: String,
    /// Bobot terhadap skor total, 1–5 (default 1).
    #[serde(default)]
    pub weight: Option<i16>,
}

/// Rule yang sudah divalidasi & dilengkapi default, siap disimpan.
#[derive(Debug, Clone)]
pub struct NewScreenerRule {
    pub flex_param_id: Uuid,
    pub indicator_code: String,
    pub params: Value,
    pub condition: String,
    pub weight: i16,
    pub sort_order: i16,
}

/// Katalog indikator untuk prompt asisten AI (hanya yang sudah punya rumus di `src/indicator/`).
#[derive(Debug, Serialize)]
pub struct IndicatorOption {
    pub flex_param_id: Uuid,
    pub code: String,
    pub description: Option<String>,
    pub conditions: Vec<ConditionOption>,
    pub params: Vec<ParamOption>,
}

#[derive(Debug, Serialize)]
pub struct ConditionOption {
    pub code: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ParamOption {
    pub name: &'static str,
    pub label: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// `true` = harus bilangan bulat (mis. period).
    pub integer: bool,
}

#[derive(Debug, Serialize)]
pub struct RuleScore {
    pub rule_id: Uuid,
    pub indicator_code: String,
    pub condition: String,
    pub weight: i16,
    pub score: f64,
    /// Nilai indikator di candle terakhir, mis. `{"rsi": 27.41}`.
    pub values: Value,
}
