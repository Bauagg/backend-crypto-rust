use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::StrategyVersion;
use crate::utils::app_error::AppError;

/// Nomor versi berikutnya 1 strategi. Bentrok 2 request bersamaan ditolak UNIQUE
/// (strategy_id, version_no).
pub async fn next_version_no(tx: &mut Transaction<'_, Postgres>, strategy_id: Uuid) -> Result<i32, AppError> {
    let next: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version_no), 0) + 1 FROM strategy_versions WHERE strategy_id = $1",
    )
    .bind(strategy_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(next)
}

pub struct NewVersion<'a> {
    pub strategy_id: Uuid,
    pub version_no: i32,
    pub language: &'a str,
    pub source_code: &'a str,
    pub inputs: &'a Value,
    pub output_type: &'a str,
    pub timeframe: &'a str,
    pub checksum: &'a str,
    pub change_note: Option<&'a str>,
    pub ai_conversation: Option<&'a Value>,
    pub entry_score: Decimal,
    pub exit_score: Decimal,
    pub position_percent: Decimal,
    pub max_positions: i16,
    pub coin_scope: &'a str,
    pub symbols: &'a [String],
}

pub async fn create_version(
    tx: &mut Transaction<'_, Postgres>,
    version: &NewVersion<'_>,
    actor: &str,
) -> Result<StrategyVersion, AppError> {
    let created = sqlx::query_as::<_, StrategyVersion>(
        r#"
        INSERT INTO strategy_versions
            (strategy_id, version_no, language, source_code, inputs, output_type, timeframe,
             checksum, change_note, ai_conversation, created_by, entry_score, exit_score,
             position_percent, max_positions, coin_scope, symbols)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
        RETURNING *
        "#,
    )
    .bind(version.strategy_id)
    .bind(version.version_no)
    .bind(version.language)
    .bind(version.source_code)
    .bind(version.inputs)
    .bind(version.output_type)
    .bind(version.timeframe)
    .bind(version.checksum)
    .bind(version.change_note)
    .bind(version.ai_conversation)
    .bind(actor)
    .bind(version.entry_score)
    .bind(version.exit_score)
    .bind(version.position_percent)
    .bind(version.max_positions)
    .bind(version.coin_scope)
    .bind(version.symbols)
    .fetch_one(&mut **tx)
    .await?;
    Ok(created)
}

pub async fn find_version_by_id(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Option<StrategyVersion>, AppError> {
    let version = sqlx::query_as::<_, StrategyVersion>("SELECT * FROM strategy_versions WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(version)
}

/// Semua versi 1 strategi, terbaru dulu.
pub async fn find_versions_of_strategy(
    tx: &mut Transaction<'_, Postgres>,
    strategy_id: Uuid,
) -> Result<Vec<StrategyVersion>, AppError> {
    let versions = sqlx::query_as::<_, StrategyVersion>(
        "SELECT * FROM strategy_versions WHERE strategy_id = $1 ORDER BY version_no DESC",
    )
    .bind(strategy_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(versions)
}

/// Beberapa versi sekaligus (mis. versi aktif tiap strategi di daftar).
pub async fn find_versions_by_ids(tx: &mut Transaction<'_, Postgres>, ids: &[Uuid]) -> Result<Vec<StrategyVersion>, AppError> {
    let versions = sqlx::query_as::<_, StrategyVersion>("SELECT * FROM strategy_versions WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(&mut **tx)
        .await?;
    Ok(versions)
}
