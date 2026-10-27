use axum::{
    extract::{Path, Query, State},
    response::Response,
    Extension, Json,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::service::{
    ai_chat_service, create_strategy_service, create_version_service, delete_strategy_service,
    get_my_strategies_service, get_strategy_results_service,
    get_strategy_service, get_versions_service, update_strategy_service, validate_script_service,
};
use super::types::{CreateStrategyInput, RunQuery, StrategyAiChatInput, UpdateStrategyInput, ValidateScriptInput};
use crate::services::strategy_versions::types::VersionInput;
use crate::strategy_engine;
use crate::utils::api_response::{created, success};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// Strategi milik user yang login. Contoh: GET /api/strategies
pub async fn get_my_strategies(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let strategies = get_my_strategies_service(&pool, claims.user_uuid()?).await?;
    Ok(success(strategies, "Berhasil mengambil strategi"))
}

/// Dokumentasi API script (variabel, fungsi, format hasil) untuk editor di FE.
/// Contoh: GET /api/strategies/reference
pub async fn get_script_reference() -> Result<Response, AppError> {
    Ok(success(
        json!({
            "language": strategy_engine::LANGUAGE,
            "timeframe": strategy_engine::TIMEFRAME,
            "max_source_chars": strategy_engine::MAX_SOURCE_CHARS,
            "reference": strategy_engine::API_REFERENCE,
        }),
        "Berhasil mengambil referensi script",
    ))
}

/// Buat strategi + versi pertama. Body `{ "name", "description"?, "kind"?, "source_code",
/// "inputs"?, "change_note"?, "ai_conversation"? }`. Contoh: POST /api/strategies
pub async fn create_strategy(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<CreateStrategyInput>,
) -> Result<Response, AppError> {
    let strategy = create_strategy_service(&pool, claims.user_uuid()?, &claims.email, input).await?;
    Ok(created(strategy, "Strategi berhasil dibuat"))
}

/// Coba script tanpa menyimpan. Body `{ "source_code", "inputs"?, "input_values"? }`.
/// Contoh: POST /api/strategies/validate
pub async fn validate_script(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<ValidateScriptInput>,
) -> Result<Response, AppError> {
    claims.user_uuid()?;
    let result = validate_script_service(&pool, input).await?;
    let message = if result.valid { "Script valid" } else { "Script belum valid" };
    Ok(success(result, message))
}

/// Chat dengan asisten AI penulis script. Body `{ "messages": [...], "strategy_id"? }`.
/// Contoh: POST /api/strategies/ai-chat
pub async fn ai_chat(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<StrategyAiChatInput>,
) -> Result<Response, AppError> {
    let result = ai_chat_service(&pool, claims.user_uuid()?, input).await?;
    Ok(success(result, "Berhasil mendapat balasan asisten AI"))
}

pub async fn get_strategy(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let strategy = get_strategy_service(&pool, id, claims.user_uuid()?).await?;
    Ok(success(strategy, "Berhasil mengambil strategi"))
}

/// Ubah nama/deskripsi/jenis/status/visibility. Isi script diubah lewat versi baru.
pub async fn update_strategy(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateStrategyInput>,
) -> Result<Response, AppError> {
    let strategy = update_strategy_service(&pool, id, claims.user_uuid()?, &claims.email, input).await?;
    Ok(success(strategy, "Strategi berhasil diubah"))
}

pub async fn delete_strategy(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    delete_strategy_service(&pool, id, claims.user_uuid()?, &claims.email).await?;
    Ok(success(serde_json::Value::Null, "Strategi berhasil dihapus"))
}

pub async fn get_versions(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let versions = get_versions_service(&pool, id, claims.user_uuid()?).await?;
    Ok(success(versions, "Berhasil mengambil versi strategi"))
}

/// Simpan versi baru script. Body `{ "source_code", "inputs"?, "change_note"?, "ai_conversation"? }`.
/// Contoh: POST /api/strategies/:id/versions
pub async fn create_version(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Json(input): Json<VersionInput>,
) -> Result<Response, AppError> {
    let version = create_version_service(&pool, id, claims.user_uuid()?, &claims.email, input).await?;
    Ok(created(version, "Versi strategi berhasil disimpan"))
}

/// Jalankan versi aktif strategi ke semua coin. Contoh: GET /api/strategies/:id/results?limit=10
pub async fn get_strategy_results(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Query(query): Query<RunQuery>,
) -> Result<Response, AppError> {
    let result = get_strategy_results_service(&pool, id, claims.user_uuid()?, query).await?;
    Ok(success(result, "Berhasil menjalankan strategi"))
}
