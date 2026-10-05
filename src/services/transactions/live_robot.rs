//! Robot trading akun LIVE: tiap hari untuk semua user yang `is_robot_platform_active` dan punya
//! API key/secret, minta sinyal ke API strategi Python (`POST /signal`) lalu kirim order MARKET
//! sungguhan ke Binance (atau Spot Testnet kalau `ACCOUNT_API_BASE_URL` diarahkan ke sana). Yang
//! dicatat di `trade_positions` adalah hasil eksekusi asli: qty, harga rata-rata, fee per fill,
//! plus respons Binance apa adanya sebagai bukti order. Alur mengikuti `API_RUST.md` bagian 4,
//! sama dengan robot demo.
//!
//! Coin milik robot = posisi LIVE BOT yang OPEN, dibatasi saldo `free` di Binance (kalau user
//! menjual sendiri lewat aplikasi Binance, robot tidak menjual coin yang sudah tidak ada). Coin lain
//! di akun yang dibeli user sendiri tidak dikirim ke Python dan tidak pernah dijual robot.
//!
//! Order ke exchange tidak bisa di-rollback, jadi tiap order yang berhasil langsung dicatat dalam
//! transaksi DB-nya sendiri; kalau pencatatan gagal, respons order ditulis ke log error untuk
//! dicocokkan manual. Order yang ditolak dilewati (dicatat di log), order lain tetap jalan.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate, Utc};
use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::Value;
use sqlx::PgPool;

use super::model::TradePosition;
use super::repository::{create_trade_positions_bulk, find_open_trade_positions};
use super::robot_common::{
    buy_retry_factor, close_positions_fifo, floor_to_step, min_account_value, to_decimal, to_f64,
    SellFill, SOURCE, SYMBOL_TYPE_PARAM, SYSTEM_ACTOR, USDT_IDR_SYMBOL,
};
use super::types::NewTradePosition;
use crate::clients::binance::{self, OrderAmount, OrderFill, OrderResult, TradingRules};
use crate::clients::strategy_api::{self, SignalOrder, SignalRequest};
use crate::services::flex_params::repository::find_flex_params_by_type;
use crate::services::users::model::User;
use crate::services::users::repository::{find_platform_robot_users, update_platform_robot_state};
use crate::services::users::types::is_supported_platform;
use crate::utils::app_error::AppError;
use crate::utils::crypto::decrypt;

const ACCOUNT_MODE: &str = "LIVE";
const QUOTE_ASSET: &str = "USDT";

/// Ringkasan 1 putaran robot live, untuk log.
#[derive(Debug, Default)]
pub struct LiveRobotSummary {
    pub processed: usize,
    pub deactivated_low_balance: usize,
    pub deactivated_kill_switch: usize,
    /// User yang gagal diproses sebelum order apa pun dikirim (akan dicoba lagi).
    pub errors: usize,
    pub orders_filled: usize,
    pub orders_failed: usize,
    pub positions_opened: usize,
    pub positions_closed: usize,
}

pub enum LiveRobotRun {
    Done(LiveRobotSummary),
    /// Candle kemarin belum masuk DB (data basi) — jangan trading, coba lagi nanti.
    StaleData { candle_date: String },
}

#[derive(Default)]
struct OrderStats {
    filled: usize,
    failed: usize,
    opened: usize,
    closed: usize,
}

enum UserOutcome {
    Traded(OrderStats),
    KillSwitch(OrderStats),
    LowBalance,
    StaleData(String),
}

struct Credentials {
    api_key: String,
    api_secret: String,
}

/// Simbol pair USDT -> aset dasarnya (`BTCUSDT` -> `BTC`).
fn base_asset(symbol: &str) -> &str {
    symbol.strip_suffix(QUOTE_ASSET).unwrap_or(symbol)
}

/// Jalankan robot live untuk semua user aktif yang belum diproses hari ini (UTC).
pub async fn run_live_robot(pool: &PgPool) -> Result<LiveRobotRun, AppError> {
    let today = Utc::now().date_naive();
    let yesterday = today - Duration::days(1);

    let users: Vec<User> = {
        let mut tx = pool
            .begin()
            .await
            .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))?;
        let users = find_platform_robot_users(&mut tx).await?;
        let _ = tx.commit().await;
        users
            .into_iter()
            .filter(|u| u.platform_robot_last_run_date != Some(today))
            .filter(|u| u.platform.as_deref().is_some_and(is_supported_platform))
            .collect()
    };

    let mut summary = LiveRobotSummary::default();
    if users.is_empty() {
        return Ok(LiveRobotRun::Done(summary));
    }

    // Kurs dari pasar asli; harga untuk menilai saldo & order dari exchange tempat akun berada.
    let market_prices = binance::get_all_prices().await?;
    let usdt_idr_rate = market_prices
        .get(USDT_IDR_SYMBOL)
        .copied()
        .filter(|rate| !rate.is_zero())
        .ok_or_else(|| AppError::Internal("Kurs USDTIDR tidak tersedia".to_string()))?;
    let account_prices = if binance::is_testnet() {
        binance::get_account_prices().await?
    } else {
        market_prices
    };

    for user in &users {
        match process_user(pool, user, &account_prices, usdt_idr_rate, today, yesterday).await {
            Ok(UserOutcome::Traded(stats)) => {
                summary.processed += 1;
                add_stats(&mut summary, &stats);
            }
            Ok(UserOutcome::KillSwitch(stats)) => {
                summary.processed += 1;
                summary.deactivated_kill_switch += 1;
                add_stats(&mut summary, &stats);
                tracing::warn!("Robot live {}: kill switch aktif, robot dimatikan", user.email);
            }
            Ok(UserOutcome::LowBalance) => {
                summary.deactivated_low_balance += 1;
                tracing::warn!("Robot live {}: saldo tidak cukup, robot dimatikan", user.email);
            }
            // Data candle sama untuk semua user — kalau basi untuk 1 user, basi untuk semua.
            Ok(UserOutcome::StaleData(candle_date)) => {
                return Ok(LiveRobotRun::StaleData { candle_date });
            }
            Err(err) => {
                summary.errors += 1;
                tracing::error!("Robot live {} gagal: {err:?}", user.email);
            }
        }
    }

    Ok(LiveRobotRun::Done(summary))
}

fn add_stats(summary: &mut LiveRobotSummary, stats: &OrderStats) {
    summary.orders_filled += stats.filled;
    summary.orders_failed += stats.failed;
    summary.positions_opened += stats.opened;
    summary.positions_closed += stats.closed;
}

fn decrypt_credentials(user: &User) -> Result<Credentials, AppError> {
    let (Some(key), Some(secret)) = (&user.api_key, &user.api_secret) else {
        return Err(AppError::BadRequest("API key/secret belum diisi".to_string()));
    };
    Ok(Credentials {
        api_key: decrypt(key)?,
        api_secret: decrypt(secret)?,
    })
}

async fn process_user(
    pool: &PgPool,
    user: &User,
    prices: &HashMap<String, Decimal>,
    usdt_idr_rate: Decimal,
    today: NaiveDate,
    yesterday: NaiveDate,
) -> Result<UserOutcome, AppError> {
    let credentials = decrypt_credentials(user)?;

    let (mut positions, coin_ids) = {
        let mut tx = begin(pool).await?;
        let positions = find_open_trade_positions(&mut tx, user.id, ACCOUNT_MODE, SOURCE).await?;
        let coin_ids: HashMap<String, uuid::Uuid> =
            find_flex_params_by_type(&mut tx, SYMBOL_TYPE_PARAM, false)
                .await?
                .into_iter()
                .map(|p| (p.value_param, p.id))
                .collect();
        let _ = tx.commit().await;
        (positions, coin_ids)
    };

    let free: HashMap<String, Decimal> =
        binance::get_account_balances(&credentials.api_key, &credentials.api_secret)
            .await?
            .into_iter()
            .map(|b| (b.asset, b.free))
            .collect();
    let free_of = |asset: &str| free.get(asset).copied().unwrap_or_default();

    // Coin milik robot: total posisi OPEN per simbol, tapi tidak lebih dari yang benar-benar ada.
    let mut holdings: HashMap<String, Decimal> = HashMap::new();
    for position in &positions {
        *holdings.entry(position.symbol.clone()).or_default() += position.quantity;
    }
    for (symbol, quantity) in holdings.iter_mut() {
        let available = free_of(base_asset(symbol));
        if *quantity > available {
            tracing::warn!(
                "Robot live {}: posisi {symbol} {quantity} > saldo Binance {available}, dipakai saldo Binance",
                user.email
            );
            *quantity = available;
        }
    }

    let mut cash = free_of(QUOTE_ASSET);
    let price_of = |symbol: &str| prices.get(symbol).copied().unwrap_or_default();
    let total_value = cash + holdings.iter().map(|(s, q)| *q * price_of(s)).sum::<Decimal>();

    // Saldo tidak cukup untuk membuat order apa pun -> lewati & matikan robot.
    if total_value < min_account_value() {
        let mut tx = begin(pool).await?;
        update_platform_robot_state(&mut tx, user.id, user.platform_initial_capital, today, false).await?;
        commit(tx).await?;
        return Ok(UserOutcome::LowBalance);
    }

    let initial_capital = user.platform_initial_capital.unwrap_or(total_value);
    let request = SignalRequest {
        modal: to_f64(total_value * usdt_idr_rate),
        modal_awal: Some(to_f64(initial_capital * usdt_idr_rate)),
        kurs_usdt_idr: to_f64(usdt_idr_rate),
        posisi: holdings
            .iter()
            .filter(|(_, q)| !q.is_zero())
            .map(|(s, q)| (s.clone(), to_f64(*q)))
            .collect(),
        cash_usdt: to_f64(cash),
    };
    let signal = strategy_api::post_signal(&request).await?;

    for note in &signal.catatan {
        tracing::info!("Robot live {} catatan /signal: {note}", user.email);
    }
    if signal.tanggal_candle != yesterday.to_string() {
        return Ok(UserOutcome::StaleData(signal.tanggal_candle));
    }

    let kill_switch = signal.kill_switch.aktif;
    let mut orders = signal.order.unwrap_or_default();
    tracing::info!(
        "Robot live {}: strategi {}, status {}, perlu_rebalance {}, {} order",
        user.email,
        signal.strategi,
        signal.status,
        signal.perlu_rebalance,
        orders.len()
    );
    orders.sort_by_key(|o| o.urutan);
    if !(kill_switch || signal.perlu_rebalance) {
        orders.clear();
    }

    let symbols: Vec<String> = orders.iter().map(|o| o.symbol.clone()).collect();
    let rules = binance::get_account_trading_rules(&symbols).await?;

    let mut stats = OrderStats::default();
    let context = OrderContext {
        pool,
        user,
        credentials: &credentials,
        prices,
        strategy: &signal.strategi,
        usdt_idr_rate,
        account_capital: total_value,
    };

    for order in &orders {
        let (price, Some(&rule)) = (price_of(&order.symbol), rules.get(&order.symbol)) else {
            tracing::warn!("Robot live {}: aturan order {} tidak ada, dilewati", user.email, order.symbol);
            continue;
        };
        if price.is_zero() {
            tracing::warn!("Robot live {}: harga {} tidak ada, dilewati", user.email, order.symbol);
            continue;
        }

        let result = match order.side.as_str() {
            "SELL" => context
                .sell(order, price, rule, &mut positions, &mut holdings, &mut cash, &mut stats)
                .await,
            "BUY" => match coin_ids.get(&order.symbol) {
                Some(&flex_param_id) => {
                    context.buy(order, rule, flex_param_id, &mut cash, &mut stats).await
                }
                None => {
                    tracing::warn!("Robot live {}: {} tidak ada di daftar coin, BUY dilewati", user.email, order.symbol);
                    Ok(())
                }
            },
            other => {
                tracing::warn!("Robot live {}: side '{other}' tidak dikenal", user.email);
                Ok(())
            }
        };
        if let Err(err) = result {
            stats.failed += 1;
            tracing::error!("Robot live {}: {} {} gagal: {err:?}", user.email, order.side, order.symbol);
        }
    }

    // Tanggal proses tetap dicatat walau ada order yang gagal — supaya order uang asli tidak
    // dikirim ulang berkali-kali hari ini. Sisa selisih dikejar /signal besok (posisi tidak sesuai
    // target -> Python memberi order lagi).
    let mut tx = begin(pool).await?;
    update_platform_robot_state(&mut tx, user.id, Some(initial_capital), today, !kill_switch).await?;
    commit(tx).await?;

    Ok(if kill_switch {
        UserOutcome::KillSwitch(stats)
    } else {
        UserOutcome::Traded(stats)
    })
}

async fn begin(pool: &PgPool) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, AppError> {
    pool.begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))
}

async fn commit(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), AppError> {
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))
}

/// Fee semua fill dalam USDT: fee dalam coin yang dibeli dikali harga isi, fee BNB (kalau user
/// mengaktifkan bayar fee pakai BNB) dikali harga BNBUSDT.
fn fee_in_usdt(fills: &[OrderFill], symbol: &str, avg_price: Decimal, prices: &HashMap<String, Decimal>) -> Decimal {
    fills
        .iter()
        .map(|fill| {
            let asset = fill.commission_asset.as_str();
            if asset == QUOTE_ASSET {
                fill.commission
            } else if asset == base_asset(symbol) {
                fill.commission * avg_price
            } else {
                let rate = prices.get(&format!("{asset}{QUOTE_ASSET}")).copied().unwrap_or_default();
                if rate.is_zero() {
                    tracing::warn!("Fee {} {asset} tidak bisa dikonversi ke USDT", fill.commission);
                }
                fill.commission * rate
            }
        })
        .sum()
}

/// Jumlah fee yang dibayar dalam aset tertentu (mengurangi coin/USDT yang diterima).
fn commission_in(fills: &[OrderFill], asset: &str) -> Decimal {
    fills
        .iter()
        .filter(|f| f.commission_asset == asset)
        .map(|f| f.commission)
        .sum()
}

fn average_price(result: &OrderResult) -> Decimal {
    if result.executed_qty.is_zero() {
        Decimal::ZERO
    } else {
        result.quote_qty / result.executed_qty
    }
}

/// Data yang sama untuk semua order 1 user dalam 1 putaran.
struct OrderContext<'a> {
    pool: &'a PgPool,
    user: &'a User,
    credentials: &'a Credentials,
    prices: &'a HashMap<String, Decimal>,
    strategy: &'a str,
    usdt_idr_rate: Decimal,
    account_capital: Decimal,
}

impl OrderContext<'_> {
    async fn place(&self, symbol: &str, side: &str, amount: OrderAmount) -> Result<(OrderResult, Value), AppError> {
        binance::place_market_order(&self.credentials.api_key, &self.credentials.api_secret, symbol, side, amount)
            .await
    }

    /// Pencatatan gagal setelah order sudah terisi di exchange: jangan hilang — tulis lengkap ke log.
    fn log_unrecorded(&self, symbol: &str, raw: &Value, err: &AppError) {
        tracing::error!(
            "Robot live {}: order {symbol} SUDAH TERISI di Binance tapi gagal dicatat ({err:?}) — \
             cocokkan manual: {raw}",
            self.user.email
        );
    }

    /// BUY market senilai `nilai_usdt` (`quoteOrderQty`). Saldo USDT kurang -> coba ×0,99 sekali,
    /// masih kurang -> dilewati. Coin bersih = qty terisi dikurangi fee yang dipotong dari coin.
    async fn buy(
        &self,
        order: &SignalOrder,
        rule: TradingRules,
        flex_param_id: uuid::Uuid,
        cash: &mut Decimal,
        stats: &mut OrderStats,
    ) -> Result<(), AppError> {
        let mut spend = to_decimal(order.nilai_usdt);
        if spend > *cash {
            spend *= buy_retry_factor();
        }
        if spend > *cash {
            tracing::warn!("Robot live {}: USDT kurang untuk BUY {} ({spend} > {cash}), dilewati", self.user.email, order.symbol);
            return Ok(());
        }
        let spend = spend.round_dp_with_strategy(2, RoundingStrategy::ToZero);
        if spend < rule.min_notional {
            tracing::warn!("Robot live {}: BUY {} di bawah minimum order, dilewati", self.user.email, order.symbol);
            return Ok(());
        }

        let (result, raw) = self.place(&order.symbol, "BUY", OrderAmount::Quote(spend)).await?;
        stats.filled += 1;
        *cash -= result.quote_qty;

        let price = average_price(&result);
        let quantity = result.executed_qty - commission_in(&result.fills, base_asset(&order.symbol));
        let row = NewTradePosition {
            user_id: self.user.id,
            user_email: self.user.email.clone(),
            flex_param_id,
            symbol: order.symbol.clone(),
            source: SOURCE.to_string(),
            strategy: Some(self.strategy.to_string()),
            opened_at: Utc::now(),
            closed_at: None,
            holding_days: 0,
            currency: self.user.preferred_currency.clone(),
            usdt_idr_rate: self.usdt_idr_rate,
            quantity: quantity.round_dp(8),
            entry_value: result.quote_qty.round_dp(8),
            exit_value: None,
            entry_price: price.round_dp(8),
            exit_price: None,
            fee_amount: fee_in_usdt(&result.fills, &order.symbol, price, self.prices).round_dp(8),
            entry_order_id: Some(result.order_id.to_string()),
            exit_order_id: None,
            entry_order_response: Some(raw.clone()),
            exit_order_response: None,
            pnl_amount: Decimal::ZERO,
            pnl_percent: Decimal::ZERO,
            status: "OPEN".to_string(),
            result: None,
            account_capital: self.account_capital.round_dp(8),
            account_mode: ACCOUNT_MODE.to_string(),
            created_by: SYSTEM_ACTOR.to_string(),
        };

        let recorded = async {
            let mut tx = begin(self.pool).await?;
            create_trade_positions_bulk(&mut tx, std::slice::from_ref(&row)).await?;
            commit(tx).await
        }
        .await;
        match recorded {
            Ok(()) => stats.opened += 1,
            Err(err) => self.log_unrecorded(&order.symbol, &raw, &err),
        }
        Ok(())
    }

    /// SELL market: `jual_semua` = seluruh coin milik robot, selain itu `nilai_usdt / harga`;
    /// qty dibulatkan ke bawah ke `stepSize`. Hasilnya dibagi ke posisi OPEN secara FIFO.
    #[allow(clippy::too_many_arguments)]
    async fn sell(
        &self,
        order: &SignalOrder,
        price: Decimal,
        rule: TradingRules,
        positions: &mut [TradePosition],
        holdings: &mut HashMap<String, Decimal>,
        cash: &mut Decimal,
        stats: &mut OrderStats,
    ) -> Result<(), AppError> {
        let held = holdings.get(&order.symbol).copied().unwrap_or_default();
        if held.is_zero() {
            return Ok(());
        }
        let wanted = if order.jual_semua {
            held
        } else {
            (to_decimal(order.nilai_usdt) / price).min(held)
        };
        let quantity = floor_to_step(wanted, rule.step_size);
        if quantity.is_zero() || quantity * price < rule.min_notional {
            tracing::warn!("Robot live {}: SELL {} di bawah minimum order, dilewati", self.user.email, order.symbol);
            return Ok(());
        }

        let (result, raw) = self.place(&order.symbol, "SELL", OrderAmount::Base(quantity)).await?;
        stats.filled += 1;

        let avg_price = average_price(&result);
        let net_total = result.quote_qty - commission_in(&result.fills, QUOTE_ASSET);
        *cash += net_total;
        if let Some(held) = holdings.get_mut(&order.symbol) {
            *held = (*held - result.executed_qty).max(Decimal::ZERO);
        }

        let fill = SellFill {
            symbol: order.symbol.clone(),
            quantity: result.executed_qty,
            net_total,
            fee_total: fee_in_usdt(&result.fills, &order.symbol, avg_price, self.prices),
            price: avg_price.round_dp(8),
            at: Utc::now(),
            order_id: Some(result.order_id.to_string()),
            order_response: Some(raw.clone()),
        };

        let recorded = async {
            let mut tx = begin(self.pool).await?;
            let mut new_rows = Vec::new();
            let closed = close_positions_fifo(&mut tx, positions, &fill, &mut new_rows).await?;
            create_trade_positions_bulk(&mut tx, &new_rows).await?;
            commit(tx).await?;
            Ok::<usize, AppError>(closed + new_rows.len())
        }
        .await;
        match recorded {
            Ok(closed) => stats.closed += closed,
            Err(err) => self.log_unrecorded(&order.symbol, &raw, &err),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn fill(commission: &str, asset: &str) -> OrderFill {
        OrderFill {
            commission: d(commission),
            commission_asset: asset.to_string(),
        }
    }

    #[test]
    fn aset_dasar_dari_simbol() {
        assert_eq!(base_asset("BTCUSDT"), "BTC");
        assert_eq!(base_asset("DOGEUSDT"), "DOGE");
    }

    #[test]
    fn fee_beli_dipotong_dari_coin_dikonversi_ke_usdt() {
        // BUY 0.001 BTC terisi di 2 harga, fee 0,1% dalam BTC.
        let fills = vec![fill("0.0000006", "BTC"), fill("0.0000004", "BTC")];
        let prices = HashMap::new();
        assert_eq!(commission_in(&fills, "BTC"), d("0.000001"));
        assert_eq!(fee_in_usdt(&fills, "BTCUSDT", d("86040"), &prices), d("0.08604"));
    }

    #[test]
    fn fee_jual_dalam_usdt_dan_fee_bnb() {
        let mut prices = HashMap::new();
        prices.insert("BNBUSDT".to_string(), d("600"));
        let fills = vec![fill("0.086", "USDT"), fill("0.0001", "BNB")];
        assert_eq!(commission_in(&fills, "USDT"), d("0.086"));
        // 0.086 USDT + 0.0001 BNB × 600 = 0.146
        assert_eq!(fee_in_usdt(&fills, "BTCUSDT", d("86000"), &prices), d("0.146"));
    }

    #[test]
    fn harga_rata_rata_dari_total_usdt() {
        let result = OrderResult {
            order_id: 1,
            status: "FILLED".to_string(),
            executed_qty: d("0.001"),
            quote_qty: d("86.04"),
            fills: vec![],
        };
        assert_eq!(average_price(&result), d("86040"));
    }
}
