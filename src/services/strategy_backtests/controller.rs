use axum::{
    extract::{Path, Query, State},
    response::Response,
    Extension, Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::service::{get_backtest_service, get_backtest_trades_service, get_backtests_service, run_backtest_service};
use super::types::{BacktestListQuery, RunBacktestInput, TradesQuery};
use crate::utils::api_response::{created, paginated, success, PaginationParams};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// Jalankan backtest (beberapa detik). Body `{ "strategy_id", "strategy_version_id"?,
/// "input_values"? }`. Contoh: POST /api/strategy-backtests
pub async fn run_backtest(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<RunBacktestInput>,
) -> Result<Response, AppError> {
    let backtest = run_backtest_service(&pool, claims.user_uuid()?, &claims.email, input).await?;
    Ok(created(backtest, "Backtest selesai"))
}

/// Contoh: GET /api/strategy-backtests?strategy_id=...
pub async fn get_backtests(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Query(query): Query<BacktestListQuery>,
) -> Result<Response, AppError> {
    let backtests = get_backtests_service(&pool, claims.user_uuid()?, query).await?;
    Ok(success(backtests, "Berhasil mengambil backtest"))
}

/// Detail + kurva modal & drawdown harian. Contoh: GET /api/strategy-backtests/:id
pub async fn get_backtest(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let backtest = get_backtest_service(&pool, id, claims.user_uuid()?).await?;
    Ok(success(backtest, "Berhasil mengambil backtest"))
}

/// Contoh: GET /api/strategy-backtests/:id/trades?page=1&limit=50
pub async fn get_backtest_trades(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Query(query): Query<TradesQuery>,
) -> Result<Response, AppError> {
    let pagination = PaginationParams::parse(query.page, query.limit);
    let offset = (pagination.page - 1) * pagination.limit;
    let (trades, total) =
        get_backtest_trades_service(&pool, id, claims.user_uuid()?, pagination.limit, offset).await?;
    Ok(paginated(trades, total, &pagination, "Berhasil mengambil trade backtest"))
}
