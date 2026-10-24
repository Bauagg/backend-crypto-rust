use axum::{
    extract::{Path, Query, State},
    response::Response,
    Extension, Json,
};
use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use super::service::{
    get_dashboard_service, get_holding_by_symbol_service, get_holding_service,
    get_trade_position_by_id_service, get_trade_positions_service, trade_service, TradeOutcome,
};
use super::types::{
    DashboardQuery, HoldingBySymbolQuery, HoldingQuery, TradeInput, TradePositionListQuery,
};
use crate::websocket::TickerHub;
use crate::utils::api_response::{created, paginated, success, PaginationParams};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// List posisi trading milik user yang login (LIVE & DEMO), dengan search simbol, filter, sort &
/// pagination. Contoh: GET /api/transactions?search=btc&sort=opened_at&order=desc&page=1&limit=20
pub async fn get_trade_positions(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Query(query): Query<TradePositionListQuery>,
) -> Result<Response, AppError> {
    let user_id = claims.user_uuid()?;
    let pagination = PaginationParams::parse(query.page, query.limit);
    let offset = (pagination.page - 1) * pagination.limit;

    let (positions, total) =
        get_trade_positions_service(&pool, user_id, &query, pagination.limit, offset).await?;
    Ok(paginated(positions, total, &pagination, "Berhasil mengambil data posisi trading"))
}

/// Dashboard performa trading user yang login per mode akun: ringkasan, kurva modal, untung/rugi
/// per bulan & per aset, isi portofolio. Contoh: GET /api/transactions/dashboard?account_mode=LIVE
pub async fn get_dashboard(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Extension(ticker_hub): Extension<Arc<TickerHub>>,
    Query(query): Query<DashboardQuery>,
) -> Result<Response, AppError> {
    let user_id = claims.user_uuid()?;
    // Transaksi diatur di service: baca DB singkat, baru ambil harga/saldo dari exchange.
    let dashboard = get_dashboard_service(&pool, user_id, query, &ticker_hub).await?;
    Ok(success(dashboard, "Berhasil mengambil dashboard trading"))
}

/// Detail 1 posisi trading milik user yang login (+ logo coin). Contoh: GET /api/transactions/:id
pub async fn get_trade_position_by_id(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let user_id = claims.user_uuid()?;
    let position = get_trade_position_by_id_service(&pool, id, user_id).await?;
    Ok(success(position, "Berhasil mengambil detail posisi trading"))
}

/// BUY / SELL manual. Body `{ "transaction_type": "BUY"|"SELL", "flex_param_id", "account_mode",
/// "amount", "currency"? }` — sisanya (harga, kurs USDT/IDR, modal akun, posisi yang dijual FIFO)
/// diisi sistem. Contoh: POST /api/transactions
pub async fn create_trade_position(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<TradeInput>,
) -> Result<Response, AppError> {
    let user_id = claims.user_uuid()?;
    // Transaksi diatur di service: kurs, harga & saldo Binance diambil di luar transaksi DB.
    match trade_service(&pool, user_id, &claims.email, input).await? {
        TradeOutcome::Bought(position) => Ok(created(*position, "Coin berhasil dibeli")),
        TradeOutcome::Sold(result) => {
            let message = if result.remaining_quantity.is_zero() {
                "Coin berhasil dijual semua"
            } else {
                "Coin berhasil dijual sebagian"
            };
            Ok(success(*result, message))
        }
    }
}

/// Total 1 coin yang dipegang user (posisi manual OPEN) + nilainya sekarang — untuk layar Sell.
/// Contoh: GET /api/transactions/holding?flex_param_id=...&account_mode=DEMO
pub async fn get_holding(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Query(query): Query<HoldingQuery>,
) -> Result<Response, AppError> {
    let user_id = claims.user_uuid()?;
    let holding = get_holding_service(&pool, user_id, query).await?;
    Ok(success(holding, "Berhasil mengambil kepemilikan coin"))
}

/// Detail saldo 1 coin per simbol (`BTC` / `BTCUSDT`): total dari semua posisi OPEN (manual +
/// robot) + rincian tiap posisi. Contoh: GET /api/transactions/holding/BTC?account_mode=DEMO
pub async fn get_holding_by_symbol(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(symbol): Path<String>,
    Query(query): Query<HoldingBySymbolQuery>,
) -> Result<Response, AppError> {
    let user_id = claims.user_uuid()?;
    let holding = get_holding_by_symbol_service(&pool, user_id, &symbol, query).await?;
    Ok(success(holding, "Berhasil mengambil kepemilikan coin"))
}
