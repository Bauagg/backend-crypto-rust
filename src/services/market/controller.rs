use std::sync::Arc;

use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    response::Response,
    Extension,
};
use sqlx::PgPool;

use super::service::{get_klines_service, get_recommendations_service, get_symbols_service};
use super::stream_hub::KlineHub;
use super::types::{is_valid_interval, KlinesQuery, RecommendationQuery, SymbolsQuery, WsKlineQuery};
use super::websocket::relay_kline_stream;
use crate::database::RedisPool;
use crate::utils::api_response::{paginated, success, PaginationParams};
use crate::utils::app_error::AppError;

async fn begin_tx(pool: &PgPool) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, AppError> {
    pool.begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))
}

async fn commit_tx(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), AppError> {
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))
}

pub async fn get_klines(
    Extension(redis): Extension<RedisPool>,
    Query(query): Query<KlinesQuery>,
) -> Result<Response, AppError> {
    let result = get_klines_service(&redis, query).await?;
    Ok(success(result, "Berhasil mengambil data chart"))
}

pub async fn get_symbols(
    State(pool): State<PgPool>,
    Query(query): Query<SymbolsQuery>,
) -> Result<Response, AppError> {
    let pagination = PaginationParams::parse(query.page, query.limit);
    let offset = (pagination.page - 1) * pagination.limit;
    let mut tx = begin_tx(&pool).await?;

    let (symbols, total) = match get_symbols_service(
        &pool,
        &mut tx,
        query.search.as_deref(),
        pagination.limit,
        offset,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(paginated(symbols, total, &pagination, "Berhasil mengambil daftar pair"))
}

/// Daftar pantauan coin dari API strategi Python (bukan sinyal bot, tidak dieksekusi otomatis).
/// Contoh: GET /api/market/recommendations?limit=10
pub async fn get_recommendations(
    Extension(redis): Extension<RedisPool>,
    Query(query): Query<RecommendationQuery>,
) -> Result<Response, AppError> {
    let result = get_recommendations_service(&redis, query).await?;
    Ok(success(result, "Berhasil mengambil rekomendasi coin"))
}

/// Upgrade koneksi HTTP jadi WebSocket, lalu kirim candle live ke client lewat `KlineHub`
/// (1 koneksi ke exchange per simbol+interval, dipakai bersama semua client).
/// Contoh: GET /api/market/ws?symbol=BTCUSDT&interval=1m (upgrade header WebSocket).
pub async fn ws_klines(
    ws: WebSocketUpgrade,
    Extension(hub): Extension<Arc<KlineHub>>,
    Query(query): Query<WsKlineQuery>,
) -> Result<axum::response::Response, AppError> {
    if query.symbol.trim().is_empty() {
        return Err(AppError::BadRequest("symbol wajib diisi".to_string()));
    }

    let interval = query.interval.unwrap_or_else(|| "1m".to_string());
    if !is_valid_interval(&interval) {
        return Err(AppError::BadRequest("interval tidak valid".to_string()));
    }

    let symbol = query.symbol.trim().to_uppercase();

    Ok(ws.on_upgrade(move |socket| relay_kline_stream(socket, hub, symbol, interval)))
}
