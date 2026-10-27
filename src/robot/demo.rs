//! Robot trading akun DEMO: tiap hari untuk semua user yang `is_robot_demo_active`, minta sinyal ke
//! API strategi Python (`POST /signal`) lalu jalankan order-nya secara SIMULASI — tidak ada order ke
//! Binance, tapi hitungannya meniru Binance spot: harga = harga pasar terkini, qty dibulatkan ke
//! `stepSize`, order di bawah minimum notional ditolak, fee 0,1% (BUY dipotong dari coin, SELL dari
//! USDT). Alur & aturannya mengikuti `backtes-crypto/docs/API_RUST.md` bagian 4.
//!
//! Saldo demo: `users.demo_balance` = USDT bebas; coin yang dipegang = posisi DEMO BOT yang OPEN.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};

use crate::database::{begin, commit};
use crate::services::transactions::model::TradePosition;
use crate::services::transactions::repository::{create_trade_positions_bulk, find_open_positions_by_rule};
use super::common::{
    RULE_TYPE,
    robot_capital, waits_for_recovery, buy_retry_factor, close_positions_fifo, fee_rate, floor_to_step, min_account_value, to_decimal, to_f64,
    SellFill, SOURCE, SYMBOL_TYPE_PARAM, SYSTEM_ACTOR, USDT_IDR_SYMBOL,
};
use crate::services::transactions::types::NewTradePosition;
use crate::clients::binance::{self, TradingRules};
use crate::clients::strategy_api::{self, SignalOrder, SignalRequest};
use crate::services::flex_params::repository::find_flex_params_by_type;
use crate::services::users::model::User;
use crate::services::users::repository::{
    find_demo_robot_users, set_robot_awaiting_recovery, update_demo_robot_state,
};
use crate::utils::app_error::AppError;

const ACCOUNT_MODE: &str = "DEMO";

/// Ringkasan 1 putaran robot demo, untuk log.
#[derive(Debug, Default)]
pub struct DemoRobotSummary {
    pub processed: usize,
    pub deactivated_low_balance: usize,
    pub deactivated_kill_switch: usize,
    /// User yang rebalance-nya dilewati karena menunggu recovery (coin robot dijual manual).
    pub awaiting_recovery: usize,
    pub errors: usize,
    pub positions_opened: usize,
    pub positions_closed: usize,
}

pub enum DemoRobotRun {
    Done(DemoRobotSummary),
    /// Candle kemarin belum masuk DB (data basi) — jangan trading, coba lagi nanti.
    StaleData { candle_date: String },
}

enum UserOutcome {
    Traded { opened: usize, closed: usize },
    AwaitingRecovery,
    LowBalance,
    KillSwitch { opened: usize, closed: usize },
    StaleData(String),
}

/// Jalankan robot demo untuk semua user aktif yang belum diproses hari ini (UTC).
pub async fn run_demo_robot(pool: &PgPool) -> Result<DemoRobotRun, AppError> {
    let today = Utc::now().date_naive();
    let yesterday = today - Duration::days(1);

    let users: Vec<User> = {
        let mut tx = begin(pool).await?;
        let users = find_demo_robot_users(&mut tx).await?;
        commit(tx).await?;
        users
            .into_iter()
            .filter(|u| u.demo_robot_last_run_date != Some(today))
            .collect()
    };

    let mut summary = DemoRobotSummary::default();
    if users.is_empty() {
        return Ok(DemoRobotRun::Done(summary));
    }

    let prices = binance::get_all_prices().await?;
    let usdt_idr_rate = prices
        .get(USDT_IDR_SYMBOL)
        .copied()
        .filter(|rate| !rate.is_zero())
        .ok_or_else(|| AppError::Internal("Kurs USDTIDR tidak tersedia".to_string()))?;

    for user in &users {
        match process_user(pool, user, &prices, usdt_idr_rate, today, yesterday).await {
            Ok(UserOutcome::Traded { opened, closed }) => {
                summary.processed += 1;
                summary.positions_opened += opened;
                summary.positions_closed += closed;
            }
            Ok(UserOutcome::KillSwitch { opened, closed }) => {
                summary.processed += 1;
                summary.deactivated_kill_switch += 1;
                summary.positions_opened += opened;
                summary.positions_closed += closed;
                tracing::warn!("Robot demo {}: kill switch aktif, robot dimatikan", user.email);
            }
            Ok(UserOutcome::AwaitingRecovery) => {
                summary.processed += 1;
                summary.awaiting_recovery += 1;
            }
            Ok(UserOutcome::LowBalance) => {
                summary.deactivated_low_balance += 1;
                tracing::warn!("Robot demo {}: saldo tidak cukup, robot dimatikan", user.email);
            }
            // Data candle sama untuk semua user — kalau basi untuk 1 user, basi untuk semua.
            Ok(UserOutcome::StaleData(candle_date)) => {
                return Ok(DemoRobotRun::StaleData { candle_date });
            }
            Err(err) => {
                summary.errors += 1;
                tracing::error!("Robot demo {} gagal: {err:?}", user.email);
            }
        }
    }

    Ok(DemoRobotRun::Done(summary))
}

/// Proses 1 user dalam 1 transaksi DB: semua perubahan (posisi, saldo, status robot) tersimpan
/// bersamaan atau tidak sama sekali.
async fn process_user(
    pool: &PgPool,
    user: &User,
    prices: &HashMap<String, Decimal>,
    usdt_idr_rate: Decimal,
    today: NaiveDate,
    yesterday: NaiveDate,
) -> Result<UserOutcome, AppError> {
    let mut tx = begin(pool).await?;

    let mut positions = find_open_positions_by_rule(&mut tx, user.id, ACCOUNT_MODE, RULE_TYPE).await?;

    let price_of = |position: &TradePosition| {
        prices.get(&position.symbol).copied().unwrap_or(position.entry_price)
    };
    let holdings_value: Decimal = positions.iter().map(|p| p.quantity * price_of(p)).sum();
    let total_value = user.demo_balance + holdings_value;
    // Robot hanya memakai `demo_robot_capital_percent`% saldo (default 100 = seluruh saldo, sama
    // dengan aturan bot Python); sisa kas (`reserved`) tidak disentuh dan dikembalikan utuh ke
    // `demo_balance` di akhir.
    let capital = robot_capital(user.demo_balance, holdings_value, user.demo_robot_capital_percent);
    let reserved = user.demo_balance - capital.cash;
    let mut cash = capital.cash;

    // Modal robot tidak cukup untuk membuat order apa pun -> lewati & matikan robot.
    if capital.equity < min_account_value() {
        update_demo_robot_state(&mut tx, user.id, user.demo_balance, user.demo_initial_capital, today, false).await?;
        commit(tx).await?;
        return Ok(UserOutcome::LowBalance);
    }

    let initial_capital = user.demo_initial_capital.unwrap_or(capital.equity);
    let mut holdings: HashMap<String, f64> = HashMap::new();
    for position in &positions {
        *holdings.entry(position.symbol.clone()).or_default() += to_f64(position.quantity);
    }

    let request = SignalRequest {
        modal: to_f64(capital.equity * usdt_idr_rate),
        modal_awal: Some(to_f64(initial_capital * usdt_idr_rate)),
        kurs_usdt_idr: to_f64(usdt_idr_rate),
        posisi: holdings,
        cash_usdt: to_f64(cash),
    };
    let signal = strategy_api::post_signal(&request).await?;

    for note in &signal.catatan {
        tracing::info!("Robot demo {} catatan /signal: {note}", user.email);
    }
    if signal.tanggal_candle != yesterday.to_string() {
        let _ = tx.rollback().await;
        return Ok(UserOutcome::StaleData(signal.tanggal_candle));
    }

    let kill_switch = signal.kill_switch.aktif;
    let mut orders = signal.order.unwrap_or_default();
    tracing::info!(
        "Robot demo {}: strategi {}, status {}, perlu_rebalance {}, {} order",
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
    let awaiting = user.demo_robot_awaiting_recovery;
    let waiting = waits_for_recovery(awaiting, kill_switch, &signal.alasan_rebalance);
    if waiting {
        tracing::info!(
            "Robot demo {}: menunggu recovery (coin robot dijual manual) — rebalance {:?} dilewati \
             sampai rebalance terjadwal strategi",
            user.email,
            signal.alasan_rebalance
        );
        orders.clear();
    }
    // Rebalance terjadwal (bulanan / filter pasar / kill switch) dijalankan = recovery selesai.
    let recovered = awaiting && !waiting && (kill_switch || signal.perlu_rebalance);

    let symbols: Vec<String> = orders.iter().map(|o| o.symbol.clone()).collect();
    let rules = binance::get_trading_rules(&symbols).await?;
    let coin_ids: HashMap<String, uuid::Uuid> = find_flex_params_by_type(&mut tx, SYMBOL_TYPE_PARAM, false)
        .await?
        .into_iter()
        .map(|p| (p.value_param, p.id))
        .collect();

    let now = Utc::now();
    let mut new_rows: Vec<NewTradePosition> = Vec::new();
    let mut closed = 0;

    for order in &orders {
        let (Some(&price), Some(&rule)) = (prices.get(&order.symbol), rules.get(&order.symbol)) else {
            tracing::warn!("Robot demo {}: harga/aturan {} tidak ada, order dilewati", user.email, order.symbol);
            continue;
        };

        match order.side.as_str() {
            "SELL" => {
                closed += simulate_sell(
                    &mut tx, order, price, rule, now, &mut positions, &mut cash, &mut new_rows,
                )
                .await?;
            }
            "BUY" => {
                let Some(&flex_param_id) = coin_ids.get(&order.symbol) else {
                    tracing::warn!("Robot demo {}: {} tidak ada di daftar coin, BUY dilewati", user.email, order.symbol);
                    continue;
                };
                if let Some(row) = simulate_buy(
                    user, order, price, rule, now, flex_param_id, &signal.strategi, usdt_idr_rate,
                    total_value, &mut cash,
                ) {
                    new_rows.push(row);
                }
            }
            other => tracing::warn!("Robot demo {}: side '{other}' tidak dikenal", user.email),
        }
    }

    let opened = new_rows.iter().filter(|r| r.status == "OPEN").count();
    closed += new_rows.iter().filter(|r| r.status == "CLOSED").count();
    create_trade_positions_bulk(&mut tx, &new_rows).await?;

    update_demo_robot_state(
        &mut tx,
        user.id,
        (cash + reserved).round_dp(2),
        Some(initial_capital),
        today,
        !kill_switch,
    )
    .await?;
    if recovered {
        set_robot_awaiting_recovery(&mut tx, user.id, ACCOUNT_MODE, false).await?;
        tracing::info!("Robot demo {}: recovery — rebalance terjadwal dijalankan", user.email);
    }
    commit(tx).await?;

    Ok(if kill_switch {
        UserOutcome::KillSwitch { opened, closed }
    } else if waiting {
        UserOutcome::AwaitingRecovery
    } else {
        UserOutcome::Traded { opened, closed }
    })
}

/// BUY market senilai `nilai_usdt` (seperti `quoteOrderQty`): qty = nilai / harga dibulatkan ke
/// bawah ke `stepSize`, fee 0,1% dipotong dari coin yang diterima. Saldo kurang -> coba ×0,99 sekali.
#[allow(clippy::too_many_arguments)]
fn simulate_buy(
    user: &User,
    order: &SignalOrder,
    price: Decimal,
    rule: TradingRules,
    now: DateTime<Utc>,
    flex_param_id: uuid::Uuid,
    strategy: &str,
    usdt_idr_rate: Decimal,
    account_capital: Decimal,
    cash: &mut Decimal,
) -> Option<NewTradePosition> {
    let mut spend = to_decimal(order.nilai_usdt);
    if spend > *cash {
        spend *= buy_retry_factor();
    }
    if spend > *cash || price.is_zero() {
        tracing::warn!("Robot demo {}: saldo kurang untuk BUY {}, dilewati", user.email, order.symbol);
        return None;
    }

    let quantity = floor_to_step(spend / price, rule.step_size);
    let cost = quantity * price;
    if let Some(violation) = rule.market_violation(quantity, cost) {
        tracing::warn!("Robot demo {}: BUY {} melanggar aturan Binance ({violation:?}), dilewati", user.email, order.symbol);
        return None;
    }

    let fee_quantity = quantity * fee_rate();
    *cash -= cost;

    Some(NewTradePosition {
        user_id: user.id,
        user_email: user.email.clone(),
        flex_param_id,
        symbol: order.symbol.clone(),
        source: SOURCE.to_string(),
        rule_type: RULE_TYPE.to_string(),
        strategy_subscription_id: None,
        rule_id: None,
        strategy: Some(strategy.to_string()),
        opened_at: now,
        closed_at: None,
        holding_days: 0,
        currency: user.preferred_currency.clone(),
        usdt_idr_rate,
        quantity: (quantity - fee_quantity).round_dp(8),
        entry_value: cost.round_dp(8),
        exit_value: None,
        entry_price: price,
        exit_price: None,
        fee_amount: (fee_quantity * price).round_dp(8),
        entry_order_id: None,
        exit_order_id: None,
        entry_order_response: None,
        exit_order_response: None,
        pnl_amount: Decimal::ZERO,
        pnl_percent: Decimal::ZERO,
        status: "OPEN".to_string(),
        result: None,
        account_capital: account_capital.round_dp(8),
        account_mode: ACCOUNT_MODE.to_string(),
        created_by: SYSTEM_ACTOR.to_string(),
    })
}

/// SELL market: `jual_semua` = seluruh qty yang dipegang, selain itu `nilai_usdt / harga` dibulatkan
/// ke `stepSize`. Fee 0,1% dipotong dari USDT hasil jual. Posisi ditutup FIFO; posisi yang hanya
/// terjual sebagian dikurangi isinya, dan bagian yang terjual dicatat sebagai baris CLOSED baru.
/// Balik jumlah posisi yang ditutup penuh.
#[allow(clippy::too_many_arguments)]
async fn simulate_sell(
    tx: &mut Transaction<'_, Postgres>,
    order: &SignalOrder,
    price: Decimal,
    rule: TradingRules,
    now: DateTime<Utc>,
    positions: &mut [TradePosition],
    cash: &mut Decimal,
    new_rows: &mut Vec<NewTradePosition>,
) -> Result<usize, AppError> {
    let held: Decimal = positions
        .iter()
        .filter(|p| p.symbol == order.symbol)
        .map(|p| p.quantity)
        .sum();
    if held.is_zero() || price.is_zero() {
        return Ok(0);
    }

    let sell_quantity = if order.jual_semua {
        held
    } else {
        floor_to_step(to_decimal(order.nilai_usdt) / price, rule.step_size).min(held)
    };
    let proceeds = sell_quantity * price;
    if let Some(violation) = rule.market_violation(sell_quantity, proceeds) {
        tracing::warn!("SELL {} melanggar aturan Binance ({violation:?}), dilewati", order.symbol);
        return Ok(0);
    }

    let exit_fee_total = proceeds * fee_rate();
    let net_total = proceeds - exit_fee_total;
    *cash += net_total;

    let fill = SellFill {
        symbol: order.symbol.clone(),
        quantity: sell_quantity,
        net_total,
        fee_total: exit_fee_total,
        price,
        at: now,
        order_id: None,
        order_response: None,
    };
    close_positions_fifo(tx, positions, &fill, new_rows).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn demo_user(balance: &str, currency: &str) -> User {
        User {
            id: uuid::Uuid::new_v4(),
            full_name: "Tes".to_string(),
            email: "tes@example.com".to_string(),
            phone: "0800".to_string(),
            password: String::new(),
            role: "user".to_string(),
            status: "active".to_string(),
            platform: None,
            api_key: None,
            api_secret: None,
            photo_id: None,
            photo_url: None,
            demo_balance: d(balance),
            is_robot_demo_active: true,
            is_robot_platform_active: false,
            preferred_currency: currency.to_string(),
            demo_initial_capital: None,
            demo_robot_last_run_date: None,
            platform_initial_capital: None,
            platform_robot_last_run_date: None,
            demo_robot_awaiting_recovery: false,
            platform_robot_awaiting_recovery: false,
            demo_robot_capital_percent: Decimal::ONE_HUNDRED,
            platform_robot_capital_percent: Decimal::ONE_HUNDRED,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn buy_order(symbol: &str, nilai: f64) -> SignalOrder {
        SignalOrder { urutan: 1, symbol: symbol.to_string(), side: "BUY".to_string(), nilai_usdt: nilai, jual_semua: false }
    }

    #[test]
    fn buy_seperti_binance_qty_dibulatkan_fee_dari_coin() {
        let user = demo_user("1000", "IDR");
        let mut cash = d("1000");
        let rule = TradingRules { step_size: d("0.00001"), min_qty: d("0.00001"), max_qty: Some(d("9000")), min_notional: d("5"), max_notional: None };
        let row = simulate_buy(&user, &buy_order("BTCUSDT", 50.0), d("86150"), rule, Utc::now(),
            uuid::Uuid::new_v4(), "V23", d("17889"), d("1000"), &mut cash).unwrap();

        // 50 / 86150 = 0.000580383... -> 0.00058 BTC; biaya = 0.00058 × 86150 = 49.967
        assert_eq!(row.entry_value, d("49.967"));
        assert_eq!(cash, d("1000") - d("49.967"));
        // fee 0,1% dari coin: diterima 0.00058 - 0.00000058
        assert_eq!(row.quantity, d("0.00057942"));
        assert_eq!(row.fee_amount, d("0.049967"));
        assert_eq!((row.currency.as_str(), row.status.as_str(), row.source.as_str()), ("IDR", "OPEN", "BOT"));
        assert_eq!(row.strategy.as_deref(), Some("V23"));
    }

    #[test]
    fn buy_saldo_kurang_coba_99_persen_lalu_skip() {
        let user = demo_user("49.6", "USDT");
        let rule = TradingRules { step_size: d("0.00001"), min_qty: d("0.00001"), max_qty: Some(d("9000")), min_notional: d("5"), max_notional: None };
        // 50 > 49.6 -> dicoba 49.5 (×0,99) -> cukup
        let mut cash = d("49.6");
        assert!(simulate_buy(&user, &buy_order("BTCUSDT", 50.0), d("86150"), rule, Utc::now(),
            uuid::Uuid::new_v4(), "V23", d("17889"), d("49.6"), &mut cash).is_some());
        // 50 × 0,99 = 49.5 > 40 -> dilewati, saldo utuh
        let mut cash = d("40");
        assert!(simulate_buy(&user, &buy_order("BTCUSDT", 50.0), d("86150"), rule, Utc::now(),
            uuid::Uuid::new_v4(), "V23", d("17889"), d("40"), &mut cash).is_none());
        assert_eq!(cash, d("40"));
    }

    #[test]
    fn buy_di_bawah_minimum_notional_skip() {
        let user = demo_user("1000", "USDT");
        let mut cash = d("1000");
        let rule = TradingRules { step_size: d("0.00001"), min_qty: d("0.00001"), max_qty: Some(d("9000")), min_notional: d("5"), max_notional: None };
        assert!(simulate_buy(&user, &buy_order("BTCUSDT", 4.0), d("86150"), rule, Utc::now(),
            uuid::Uuid::new_v4(), "V23", d("17889"), d("1000"), &mut cash).is_none());
        assert_eq!(cash, d("1000"));
    }
}
