use axum::{
    extract::{Path, Query, State},
    response::Response,
    Extension, Json,
};
use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use super::service::{
    create_trade_position_service, get_dashboard_service, get_trade_position_by_id_service,
    get_trade_positions_service,
};
use super::types::{CreateTradePositionInput, DashboardQuery, TradePositionListQuery};
use crate::services::market::ticker_hub::TickerHub;
use crate::utils::api_response::{created, paginated, success, PaginationParams};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

fn parse_user_id(claims: &JwtClaims) -> Result<Uuid, AppError> {
    Uuid::parse_str(&claims.user_id).map_err(|_| AppError::Unauthorized("Token tidak valid".to_string()))
}

/// List posisi trading milik user yang login (LIVE & DEMO), dengan search simbol, filter, sort &
/// pagination. Contoh: GET /api/transactions?search=btc&sort=opened_at&order=desc&page=1&limit=20
pub async fn get_trade_positions(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Query(query): Query<TradePositionListQuery>,
) -> Result<Response, AppError> {
    let user_id = parse_user_id(&claims)?;
    let pagination = PaginationParams::parse(query.page, query.limit);
    let offset = (pagination.page - 1) * pagination.limit;

    let mut tx = pool
        .begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))?;

    let (positions, total) =
        match get_trade_positions_service(&mut tx, user_id, &query, pagination.limit, offset).await {
            Ok(result) => result,
            Err(err) => {
                let _ = tx.rollback().await;
                return Err(err);
            }
        };

    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))?;
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
    let user_id = parse_user_id(&claims)?;
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
    let user_id = parse_user_id(&claims)?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))?;

    let position = match get_trade_position_by_id_service(&mut tx, id, user_id).await {
        Ok(position) => position,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))?;
    Ok(success(position, "Berhasil mengambil detail posisi trading"))
}

/// Buka posisi baru. Body cukup `{ "flex_param_id", "entry_value", "currency", "account_mode" }` — sisanya
/// (harga masuk, kurs USDT/IDR, modal akun, simbol, waktu, status) diisi sistem. Contoh: POST /api/transactions
pub async fn create_trade_position(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<CreateTradePositionInput>,
) -> Result<Response, AppError> {
    let user_id = parse_user_id(&claims)?;
    // Transaksi diatur di service: kurs, harga & saldo Binance diambil di luar transaksi DB.
    let position = create_trade_position_service(&pool, user_id, &claims.email, input).await?;
    Ok(created(position, "Posisi trading berhasil dibuka"))
}
