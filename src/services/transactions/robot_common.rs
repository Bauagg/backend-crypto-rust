//! Bagian yang sama persis antara robot DEMO (simulasi) dan robot LIVE (order Binance asli):
//! pembulatan ke `stepSize`, hitung untung/rugi, dan penutupan posisi FIFO setelah SELL.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::{Postgres, Transaction};

use super::model::TradePosition;
use super::repository::{close_trade_position, reduce_trade_position, ClosePosition};
use super::types::NewTradePosition;

pub(super) const SOURCE: &str = "BOT";
pub(super) const SYSTEM_ACTOR: &str = "system";
pub(super) const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
pub(super) const USDT_IDR_SYMBOL: &str = "USDTIDR";

/// BUY gagal karena saldo kurang -> kecilkan sedikit lalu coba sekali lagi (API_RUST.md 4.5 poin 5).
pub(super) fn buy_retry_factor() -> Decimal {
    Decimal::new(99, 2)
}

/// Total nilai akun di bawah ini = tidak bisa membuat order sama sekali (minimum order Binance).
pub(super) fn min_account_value() -> Decimal {
    Decimal::from(5)
}

pub(super) fn to_decimal(value: f64) -> Decimal {
    Decimal::from_f64(value).unwrap_or_default()
}

pub(super) fn to_f64(value: Decimal) -> f64 {
    value.to_f64().unwrap_or_default()
}

/// Bulatkan ke bawah ke kelipatan `step` (aturan LOT_SIZE Binance).
pub(super) fn floor_to_step(value: Decimal, step: Decimal) -> Decimal {
    if step.is_zero() {
        return value;
    }
    (value / step).floor() * step
}

pub(super) fn percent(pnl: Decimal, base: Decimal) -> Decimal {
    if base.is_zero() {
        return Decimal::ZERO;
    }
    (pnl / base * Decimal::ONE_HUNDRED).round_dp(4)
}

pub(super) fn result_of(pnl: Decimal) -> String {
    if pnl >= Decimal::ZERO { "PROFIT" } else { "LOSS" }.to_string()
}

/// Hasil 1 SELL (simulasi atau eksekusi asli) yang akan dibagi ke posisi-posisi OPEN.
pub(super) struct SellFill {
    pub symbol: String,
    /// Jumlah coin yang terjual.
    pub quantity: Decimal,
    /// USDT bersih yang diterima (sudah dipotong fee jual).
    pub net_total: Decimal,
    /// Fee jual dalam USDT.
    pub fee_total: Decimal,
    /// Harga jual (rata-rata kalau order terisi di beberapa harga).
    pub price: Decimal,
    pub at: DateTime<Utc>,
    /// Bukti order Binance (LIVE); `None` untuk DEMO.
    pub order_id: Option<String>,
    pub order_response: Option<Value>,
}

/// Bagi hasil SELL ke posisi OPEN simbol itu secara FIFO (terlama dulu). Posisi yang terjual
/// seluruhnya ditutup; posisi yang hanya terjual sebagian dikurangi isinya, dan bagian yang terjual
/// dicatat sebagai baris CLOSED baru di `new_rows`. Balik jumlah posisi yang ditutup penuh.
pub(super) async fn close_positions_fifo(
    tx: &mut Transaction<'_, Postgres>,
    positions: &mut [TradePosition],
    fill: &SellFill,
    new_rows: &mut Vec<NewTradePosition>,
) -> Result<usize, crate::utils::app_error::AppError> {
    if fill.quantity.is_zero() {
        return Ok(0);
    }

    let mut remaining = fill.quantity;
    let mut closed = 0;
    for position in positions
        .iter_mut()
        .filter(|p| p.symbol == fill.symbol && !p.quantity.is_zero())
    {
        if remaining.is_zero() {
            break;
        }
        let take = remaining.min(position.quantity);
        let share = take / fill.quantity;
        let exit_value = fill.net_total * share;
        let exit_fee = fill.fee_total * share;
        let holding_days = (fill.at - position.opened_at).num_days() as i32;

        if take == position.quantity {
            let pnl = exit_value - position.entry_value;
            close_trade_position(
                tx,
                position.id,
                &ClosePosition {
                    closed_at: fill.at,
                    holding_days,
                    exit_value: exit_value.round_dp(8),
                    exit_price: fill.price,
                    exit_order_id: fill.order_id.clone(),
                    exit_order_response: fill.order_response.clone(),
                    fee_amount: (position.fee_amount + exit_fee).round_dp(8),
                    pnl_amount: pnl.round_dp(8),
                    pnl_percent: percent(pnl, position.entry_value),
                    result: result_of(pnl),
                    updated_by: SYSTEM_ACTOR.to_string(),
                },
            )
            .await?;
            position.quantity = Decimal::ZERO;
            closed += 1;
        } else {
            let fraction = take / position.quantity;
            let part_entry_value = position.entry_value * fraction;
            let part_entry_fee = position.fee_amount * fraction;
            let pnl = exit_value - part_entry_value;

            position.quantity -= take;
            position.entry_value -= part_entry_value;
            position.fee_amount -= part_entry_fee;
            reduce_trade_position(
                tx,
                position.id,
                position.quantity.round_dp(8),
                position.entry_value.round_dp(8),
                position.fee_amount.round_dp(8),
                SYSTEM_ACTOR,
            )
            .await?;

            new_rows.push(NewTradePosition {
                user_id: position.user_id,
                user_email: position.user_email.clone(),
                flex_param_id: position.flex_param_id,
                symbol: position.symbol.clone(),
                source: position.source.clone(),
                strategy: position.strategy.clone(),
                opened_at: position.opened_at,
                closed_at: Some(fill.at),
                holding_days,
                currency: position.currency.clone(),
                usdt_idr_rate: position.usdt_idr_rate,
                quantity: take.round_dp(8),
                entry_value: part_entry_value.round_dp(8),
                exit_value: Some(exit_value.round_dp(8)),
                entry_price: position.entry_price,
                exit_price: Some(fill.price),
                fee_amount: (part_entry_fee + exit_fee).round_dp(8),
                entry_order_id: position.entry_order_id.clone(),
                exit_order_id: fill.order_id.clone(),
                entry_order_response: position.entry_order_response.clone(),
                exit_order_response: fill.order_response.clone(),
                pnl_amount: pnl.round_dp(8),
                pnl_percent: percent(pnl, part_entry_value),
                status: "CLOSED".to_string(),
                result: Some(result_of(pnl)),
                account_capital: position.account_capital,
                account_mode: position.account_mode.clone(),
                created_by: SYSTEM_ACTOR.to_string(),
            });
        }
        remaining -= take;
    }

    Ok(closed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    #[test]
    fn floor_ke_step_size() {
        assert_eq!(floor_to_step(d("0.00058765"), d("0.00001")), d("0.00058"));
        assert_eq!(floor_to_step(d("123.9"), d("1")), d("123"));
    }

    #[test]
    fn persen_dan_hasil() {
        assert_eq!(percent(d("5"), d("50")), d("10"));
        assert_eq!(result_of(d("-0.01")), "LOSS");
        assert_eq!(result_of(d("0")), "PROFIT");
    }
}
