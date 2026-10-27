use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::model::StrategyVersion;
use super::repository::{create_version, find_version_by_id, next_version_no, NewVersion};
use super::types::{BotConfig, BotConfigInput, VersionInput};
use crate::database::Tx;
use crate::services::flex_params::repository::find_flex_params_by_type;
use crate::strategy_engine::{self, inputs};
use crate::utils::app_error::AppError;

/// Batas versi per strategi — mencegah tabel membengkak dari spam simpan.
const MAX_VERSIONS_PER_STRATEGY: i32 = 200;
const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
const QUOTE_ASSET: &str = "USDT";
const MAX_SYMBOLS: usize = 20;

/// Aturan robot versi pertama kalau tidak diisi.
fn default_bot_config() -> BotConfig {
    BotConfig {
        entry_score: Decimal::from(70),
        exit_score: Decimal::from(30),
        position_percent: Decimal::from(20),
        max_positions: 5,
        coin_scope: "ALL".to_string(),
        symbols: Vec::new(),
    }
}

/// Aturan robot dari body, dilengkapi `base` (versi sebelumnya / default) & dicek. Coin `SPECIFIC`
/// harus ada di daftar coin aktif.
pub async fn resolve_bot_config_in_tx(
    tx: &mut Tx<'_>,
    input: &BotConfigInput,
    base: Option<&BotConfig>,
) -> Result<BotConfig, AppError> {
    let base = base.cloned().unwrap_or_else(default_bot_config);
    let bad = |message: &str| AppError::BadRequest(message.to_string());
    let entry_score = input.entry_score.unwrap_or(base.entry_score).round_dp(2);
    let exit_score = input.exit_score.unwrap_or(base.exit_score).round_dp(2);
    let position_percent = input.position_percent.unwrap_or(base.position_percent).round_dp(2);
    let max_positions = input.max_positions.unwrap_or(base.max_positions);
    let hundred = Decimal::ONE_HUNDRED;
    if !(Decimal::ZERO..=hundred).contains(&entry_score) || !(Decimal::ZERO..=hundred).contains(&exit_score) {
        return Err(bad("entry_score & exit_score harus 0–100"));
    }
    if exit_score >= entry_score {
        return Err(bad("exit_score harus lebih kecil dari entry_score"));
    }
    if !(Decimal::ONE..=hundred).contains(&position_percent) {
        return Err(bad("position_percent harus 1–100"));
    }
    if !(1..=50).contains(&max_positions) {
        return Err(bad("max_positions harus 1–50"));
    }

    let coin_scope = input.coin_scope.as_deref().map(|s| s.trim().to_uppercase()).unwrap_or(base.coin_scope.clone());
    let symbols = match coin_scope.as_str() {
        "ALL" => Vec::new(),
        "SPECIFIC" => {
            let requested = input.symbols.clone().unwrap_or(base.symbols.clone());
            let mut symbols: Vec<String> = requested
                .iter()
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())
                .map(|s| if s.ends_with(QUOTE_ASSET) { s } else { format!("{s}{QUOTE_ASSET}") })
                .collect();
            symbols.sort();
            symbols.dedup();
            if symbols.is_empty() || symbols.len() > MAX_SYMBOLS {
                return Err(bad("coin_scope SPECIFIC wajib berisi 1–20 coin di symbols"));
            }
            let active: Vec<String> = find_flex_params_by_type(tx, SYMBOL_TYPE_PARAM, true)
                .await?
                .into_iter()
                .map(|p| p.value_param)
                .collect();
            if let Some(unknown) = symbols.iter().find(|s| !active.contains(s)) {
                return Err(AppError::BadRequest(format!("Coin {unknown} tidak ada di daftar coin aktif")));
            }
            symbols
        }
        _ => return Err(bad("coin_scope harus ALL atau SPECIFIC")),
    };

    Ok(BotConfig { entry_score, exit_score, position_percent, max_positions, coin_scope, symbols })
}

/// sha256 isi versi (bahasa + script + deklarasi inputs) — sama persis = checksum sama.
pub fn checksum(language: &str, source_code: &str, inputs: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(language.as_bytes());
    hasher.update([0]);
    hasher.update(source_code.as_bytes());
    hasher.update([0]);
    hasher.update(inputs.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

/// Validasi isi versi tanpa menyimpan: script lolos compile & deklarasi inputs benar. Balik
/// deklarasi inputs yang sudah dinormalisasi (JSON).
pub fn validate_version_input(input: &VersionInput) -> Result<serde_json::Value, AppError> {
    let specs = inputs::parse_specs(&input.inputs).map_err(AppError::BadRequest)?;
    strategy_engine::compile(&input.source_code).map_err(AppError::BadRequest)?;
    serde_json::to_value(specs).map_err(|_| AppError::Internal("Gagal menyimpan inputs".to_string()))
}

/// Simpan versi baru 1 strategi (di transaksi pemanggil). Versi immutable — tidak ada update.
/// Aturan robot yang tidak diisi mewarisi versi aktif sebelumnya (`current_version_id`).
pub async fn create_version_in_tx(
    tx: &mut Tx<'_>,
    strategy_id: Uuid,
    current_version_id: Option<Uuid>,
    input: &VersionInput,
    actor: &str,
) -> Result<StrategyVersion, AppError> {
    let inputs = validate_version_input(input)?;
    let base = match current_version_id {
        Some(id) => find_version_by_id(tx, id).await?.map(|v| BotConfig::of(&v)),
        None => None,
    };
    let bot = resolve_bot_config_in_tx(tx, &input.bot, base.as_ref()).await?;
    let version_no = next_version_no(tx, strategy_id).await?;
    if version_no > MAX_VERSIONS_PER_STRATEGY {
        return Err(AppError::BadRequest(format!(
            "Strategi ini sudah {MAX_VERSIONS_PER_STRATEGY} versi — buat strategi baru"
        )));
    }
    let source_code = input.source_code.trim_end();
    let checksum = checksum(strategy_engine::LANGUAGE, source_code, &inputs);
    let change_note = input.change_note.as_deref().map(str::trim).filter(|n| !n.is_empty());

    create_version(
        tx,
        &NewVersion {
            strategy_id,
            version_no,
            language: strategy_engine::LANGUAGE,
            source_code,
            inputs: &inputs,
            output_type: strategy_engine::OUTPUT_TYPE,
            timeframe: strategy_engine::TIMEFRAME,
            checksum: &checksum,
            change_note,
            ai_conversation: input.ai_conversation.as_ref(),
            entry_score: bot.entry_score,
            exit_score: bot.exit_score,
            position_percent: bot.position_percent,
            max_positions: bot.max_positions,
            coin_scope: &bot.coin_scope,
            symbols: &bot.symbols,
        },
        actor,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn checksum_berubah_kalau_isi_berubah() {
        let a = checksum("RHAI", "50", &json!([]));
        assert_eq!(a, checksum("RHAI", "50", &json!([])));
        assert_ne!(a, checksum("RHAI", "51", &json!([])));
        assert_eq!(a.len(), 64);
    }
}
