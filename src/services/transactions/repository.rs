use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::model::{TradePosition, TradePositionWithPhoto};
use super::types::NewTradePosition;
use crate::utils::app_error::AppError;
use crate::utils::query_filter::{
    push_filters, push_order_by, FilterColumn, FilterCondition, SortCondition,
};

/// Kolom yang diisi saat insert, urutannya sama dengan bind di `create_trade_position` & `UNNEST`
/// di `create_trade_positions_bulk` (`updated_by` diisi sama dengan `created_by`).
const INSERT_COLUMNS: &str = "user_id, user_email, flex_param_id, symbol, source, strategy, \
    opened_at, closed_at, holding_days, currency, usdt_idr_rate, quantity, entry_value, exit_value, \
    entry_price, exit_price, fee_amount, entry_order_id, exit_order_id, entry_order_response, \
    exit_order_response, pnl_amount, pnl_percent, status, result, account_capital, account_mode, \
    created_by";

/// Simpan 1 posisi baru. `updated_by` diisi sama dengan `created_by`.
pub async fn create_trade_position(
    tx: &mut Transaction<'_, Postgres>,
    input: &NewTradePosition,
) -> Result<TradePosition, AppError> {
    let mut query = QueryBuilder::<Postgres>::new("INSERT INTO trade_positions (");
    query.push(INSERT_COLUMNS).push(", updated_by) VALUES (");
    let mut values = query.separated(", ");
    values
        .push_bind(input.user_id)
        .push_bind(input.user_email.clone())
        .push_bind(input.flex_param_id)
        .push_bind(input.symbol.clone())
        .push_bind(input.source.clone())
        .push_bind(input.strategy.clone())
        .push_bind(input.opened_at)
        .push_bind(input.closed_at)
        .push_bind(input.holding_days)
        .push_bind(input.currency.clone())
        .push_bind(input.usdt_idr_rate)
        .push_bind(input.quantity)
        .push_bind(input.entry_value)
        .push_bind(input.exit_value)
        .push_bind(input.entry_price)
        .push_bind(input.exit_price)
        .push_bind(input.fee_amount)
        .push_bind(input.entry_order_id.clone())
        .push_bind(input.exit_order_id.clone())
        .push_bind(input.entry_order_response.clone())
        .push_bind(input.exit_order_response.clone())
        .push_bind(input.pnl_amount)
        .push_bind(input.pnl_percent)
        .push_bind(input.status.clone())
        .push_bind(input.result.clone())
        .push_bind(input.account_capital)
        .push_bind(input.account_mode.clone())
        .push_bind(input.created_by.clone())
        .push_bind(input.created_by.clone());
    query.push(") RETURNING *");

    let position = query
        .build_query_as::<TradePosition>()
        .fetch_one(&mut **tx)
        .await?;
    Ok(position)
}

/// Simpan banyak posisi sekaligus dalam 1 query (bukan 1 query per posisi). Semua berhasil atau
/// semua gagal — kalau satu baris melanggar constraint, seluruh insert dibatalkan. Baris di-insert
/// sesuai urutan `inputs`, tapi Postgres tidak menjamin urutan `RETURNING` — cocokkan hasil lewat
/// field-nya (mis. `symbol`), jangan lewat posisi index.
pub async fn create_trade_positions_bulk(
    tx: &mut Transaction<'_, Postgres>,
    inputs: &[NewTradePosition],
) -> Result<Vec<TradePosition>, AppError> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = QueryBuilder::<Postgres>::new("INSERT INTO trade_positions (");
    query
        .push(INSERT_COLUMNS)
        .push(", updated_by) SELECT ")
        .push(INSERT_COLUMNS)
        .push(", created_by FROM UNNEST(");
    let mut arrays = query.separated(", ");
    arrays
        .push_bind(inputs.iter().map(|p| p.user_id).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.user_email.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.flex_param_id).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.symbol.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.source.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.strategy.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.opened_at).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.closed_at).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.holding_days).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.currency.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.usdt_idr_rate).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.quantity).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.entry_value).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.exit_value).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.entry_price).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.exit_price).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.fee_amount).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.entry_order_id.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.exit_order_id.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.entry_order_response.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.exit_order_response.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.pnl_amount).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.pnl_percent).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.status.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.result.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.account_capital).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.account_mode.clone()).collect::<Vec<_>>())
        .push_bind(inputs.iter().map(|p| p.created_by.clone()).collect::<Vec<_>>());
    query
        .push(") WITH ORDINALITY AS t(")
        .push(INSERT_COLUMNS)
        .push(", ord) ORDER BY ord RETURNING *");

    let positions = query
        .build_query_as::<TradePosition>()
        .fetch_all(&mut **tx)
        .await?;
    Ok(positions)
}

/// Posisi + logo coin. Dibungkus subquery bernama `trade_positions` supaya kolom yang ada di kedua
/// tabel (`id`, `created_at`, `user_id`, dst) tidak ambigu — filter & sort tetap pakai nama kolom
/// posisi apa adanya. LEFT JOIN: posisi tetap tampil walau coin-nya sudah dihapus dari daftar.
const TRADE_POSITION_WITH_PHOTO: &str = "(SELECT tp.*, fp.photo_url FROM trade_positions tp LEFT JOIN flex_params fp ON fp.id = tp.flex_param_id) AS trade_positions";

/// `WHERE` dasar: posisi milik user ini yang belum dihapus, search simbol, lalu filter dinamis.
fn push_trade_position_conditions(
    builder: &mut QueryBuilder<'_, Postgres>,
    user_id: Uuid,
    search: Option<&str>,
    filters: &[FilterCondition],
    columns: &[FilterColumn],
) -> Result<(), AppError> {
    builder
        .push(" WHERE deleted_at IS NULL AND user_id = ")
        .push_bind(user_id);
    if let Some(search) = search {
        builder
            .push(" AND symbol ILIKE '%' || ")
            .push_bind(search.to_string())
            .push(" || '%'");
    }
    push_filters(builder, filters, columns)
}

/// List posisi 1 user (+ logo coin) dengan search, filter & sort dinamis + pagination.
/// Balik `(data, total)`.
#[allow(clippy::too_many_arguments)]
pub async fn find_trade_positions_paginated(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    search: Option<&str>,
    filters: &[FilterCondition],
    sort: Option<&SortCondition>,
    columns: &[FilterColumn],
    default_order: &str,
    limit: i64,
    offset: i64,
) -> Result<(Vec<TradePositionWithPhoto>, i64), AppError> {
    let mut query = QueryBuilder::<Postgres>::new("SELECT * FROM ");
    query.push(TRADE_POSITION_WITH_PHOTO);
    push_trade_position_conditions(&mut query, user_id, search, filters, columns)?;
    push_order_by(&mut query, sort, columns, default_order)?;
    query.push(" LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
    let positions = query
        .build_query_as::<TradePositionWithPhoto>()
        .fetch_all(&mut **tx)
        .await?;

    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM ");
    count.push(TRADE_POSITION_WITH_PHOTO);
    push_trade_position_conditions(&mut count, user_id, search, filters, columns)?;
    let (total,): (i64,) = count.build_query_as().fetch_one(&mut **tx).await?;

    Ok((positions, total))
}

/// Detail 1 posisi (+ logo coin) — hanya kalau milik `user_id` dan belum dihapus. Posisi milik user
/// lain sengaja diperlakukan sama dengan tidak ada (`None`), supaya keberadaannya tidak bocor.
pub async fn find_trade_position_by_id(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<TradePositionWithPhoto>, AppError> {
    let mut query = QueryBuilder::<Postgres>::new("SELECT * FROM ");
    query
        .push(TRADE_POSITION_WITH_PHOTO)
        .push(" WHERE deleted_at IS NULL AND id = ")
        .push_bind(id)
        .push(" AND user_id = ")
        .push_bind(user_id);
    let position = query
        .build_query_as::<TradePositionWithPhoto>()
        .fetch_optional(&mut **tx)
        .await?;

    Ok(position)
}

/// Semua posisi 1 user di 1 mode akun (+ logo coin) untuk dashboard, terlama dulu.
/// `source` `None` = posisi robot & manual.
pub async fn find_trade_positions_for_dashboard(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    account_mode: &str,
    source: Option<&str>,
) -> Result<Vec<TradePositionWithPhoto>, AppError> {
    let mut query = QueryBuilder::<Postgres>::new("SELECT * FROM ");
    query
        .push(TRADE_POSITION_WITH_PHOTO)
        .push(" WHERE deleted_at IS NULL AND user_id = ")
        .push_bind(user_id)
        .push(" AND account_mode = ")
        .push_bind(account_mode.to_string());
    if let Some(source) = source {
        query.push(" AND source = ").push_bind(source.to_string());
    }
    query.push(" ORDER BY opened_at ASC, created_at ASC");

    let positions = query
        .build_query_as::<TradePositionWithPhoto>()
        .fetch_all(&mut **tx)
        .await?;
    Ok(positions)
}

/// Posisi OPEN 1 user untuk 1 mode akun & sumber, urut terlama dulu (FIFO saat dijual).
pub async fn find_open_trade_positions(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    account_mode: &str,
    source: &str,
) -> Result<Vec<TradePosition>, AppError> {
    let positions = sqlx::query_as::<_, TradePosition>(
        r#"
        SELECT * FROM trade_positions
        WHERE user_id = $1 AND account_mode = $2 AND source = $3 AND status = 'OPEN'
          AND deleted_at IS NULL
        ORDER BY opened_at ASC, created_at ASC
        "#,
    )
    .bind(user_id)
    .bind(account_mode)
    .bind(source)
    .fetch_all(&mut **tx)
    .await?;
    Ok(positions)
}

/// Data penutupan posisi (jual seluruh quantity posisi).
pub struct ClosePosition {
    pub closed_at: chrono::DateTime<chrono::Utc>,
    pub holding_days: i32,
    pub exit_value: rust_decimal::Decimal,
    pub exit_price: rust_decimal::Decimal,
    pub exit_order_id: Option<String>,
    pub exit_order_response: Option<serde_json::Value>,
    /// Total fee beli + jual.
    pub fee_amount: rust_decimal::Decimal,
    pub pnl_amount: rust_decimal::Decimal,
    pub pnl_percent: rust_decimal::Decimal,
    /// `PROFIT` | `LOSS`
    pub result: String,
    pub updated_by: String,
}

/// Tutup 1 posisi OPEN (status -> CLOSED + data jual & hasil).
pub async fn close_trade_position(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    close: &ClosePosition,
) -> Result<(), AppError> {
    let result = sqlx::query(
        r#"
        UPDATE trade_positions
        SET status = 'CLOSED', closed_at = $1, holding_days = $2, exit_value = $3, exit_price = $4,
            exit_order_id = $5, exit_order_response = $6, fee_amount = $7, pnl_amount = $8,
            pnl_percent = $9, result = $10, updated_by = $11, updated_at = now()
        WHERE id = $12 AND status = 'OPEN' AND deleted_at IS NULL
        "#,
    )
    .bind(close.closed_at)
    .bind(close.holding_days)
    .bind(close.exit_value)
    .bind(close.exit_price)
    .bind(&close.exit_order_id)
    .bind(&close.exit_order_response)
    .bind(close.fee_amount)
    .bind(close.pnl_amount)
    .bind(close.pnl_percent)
    .bind(&close.result)
    .bind(&close.updated_by)
    .bind(id)
    .execute(&mut **tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Posisi OPEN tidak ditemukan".to_string()));
    }
    Ok(())
}

/// Kurangi isi posisi OPEN setelah dijual sebagian (bagian yang terjual dicatat sebagai baris
/// CLOSED tersendiri oleh pemanggil).
pub async fn reduce_trade_position(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    quantity: rust_decimal::Decimal,
    entry_value: rust_decimal::Decimal,
    fee_amount: rust_decimal::Decimal,
    updated_by: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE trade_positions
        SET quantity = $1, entry_value = $2, fee_amount = $3, updated_by = $4, updated_at = now()
        WHERE id = $5 AND status = 'OPEN' AND deleted_at IS NULL
        "#,
    )
    .bind(quantity)
    .bind(entry_value)
    .bind(fee_amount)
    .bind(updated_by)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
