//! Robot strategi buatan user (`strategy_subscriptions` dengan usage BOT) — dijalankan worker
//! `strategy-bot` tiap hari setelah candle harian masuk. Terpisah dari robot Python (demo.rs /
//! live.rs): tiap bot hanya mengelola posisinya sendiri (`trade_positions.strategy_subscription_id`).
//!
//! Aturan 1 bot per hari:
//! 1. Script versi yang dipakai dijalankan ke coin cakupannya + coin yang sedang dipegang bot,
//!    dari candle close kemarin.
//! 2. JUAL seluruh coin yang skornya <= `exit_score` (atau sudah di luar cakupan strategi).
//! 3. BELI coin skor >= `entry_score` yang belum dipegang, skor tertinggi dulu, selama posisi <
//!    `max_positions`; ukuran = persen dari script (`signal(.., .., persen)`) atau
//!    `position_percent` × modal bot, tidak lebih dari kas bot. Wajib lolos aturan Binance.
//! 4. Modal bot = `capital_percent`% × (kas + nilai coin bot) — sisa saldo user tidak disentuh.
//!
//! DEMO: simulasi harga pasar + fee 0,1%, semua dalam 1 transaksi DB (gagal = batal semua).
//! LIVE: order MARKET asli; tanggal proses dicatat SEBELUM order dikirim (order uang asli
//! maksimal 1x per hari walau server restart), tiap order dicatat di transaksinya sendiri.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use super::common::{
    average_price, base_asset, close_positions_fifo, commission_in, fee_in_usdt, fee_rate, floor_to_step,
    robot_capital, SellFill, QUOTE_ASSET, SOURCE, SYSTEM_ACTOR, USDT_IDR_SYMBOL,
};
use crate::clients::binance::{self, OrderAmount, TradingRules};
use crate::database::{begin, commit};
use crate::indicator::Candle;
use crate::services::candle_ohlcv::service::{load_daily_market, to_indicator_candles};
use crate::services::fear_greed::service::recent_fear_greed_values;
use crate::services::strategies::service::usable_strategy_in_tx;
use crate::services::strategy_subscriptions::model::StrategySubscription;
use crate::services::strategy_subscriptions::repository::{find_bots_to_run, mark_bot_run};
use crate::services::strategy_versions::model::StrategyVersion;
use crate::services::strategy_versions::repository::find_version_by_id;
use crate::services::strategy_versions::types::BotConfig;
use crate::services::transactions::model::TradePosition;
use crate::services::transactions::repository::{create_trade_positions_bulk, find_open_positions_of_subscription};
use crate::services::transactions::types::NewTradePosition;
use crate::services::users::model::User;
use crate::services::users::repository::{credit_demo_balance, debit_demo_balance, find_user_by_id};
use crate::strategy_engine::{self, CoinData, CompiledScript, ScriptOutput};
use crate::utils::app_error::AppError;
use crate::utils::crypto::decrypt;

/// `trade_positions.rule_type` posisi bot strategi user (rule_id = versi strategi).
pub(crate) const RULE_TYPE: &str = "STRATEGY";
/// `trade_positions.strategy` (wajib untuk source BOT, maks 20 karakter).
const STRATEGY_LABEL: &str = "USER";
/// Candle per coin yang diberikan ke script (sama dengan screener/strategi).
const CANDLE_HISTORY: i64 = 300;

/// Ringkasan 1 putaran worker, untuk log.
#[derive(Debug, Default)]
pub struct StrategyBotSummary {
    pub processed: usize,
    /// Bot dilewati karena strategi diarsipkan / dihapus / hak pakai habis.
    pub skipped: usize,
    pub orders_filled: usize,
    pub orders_failed: usize,
    pub positions_opened: usize,
    pub positions_closed: usize,
    /// Bot yang gagal sebelum order apa pun dikirim (dicoba lagi putaran berikutnya).
    pub errors: usize,
}

pub enum StrategyBotRun {
    Done(StrategyBotSummary),
    /// Candle kemarin belum masuk DB — jangan trading, coba lagi nanti.
    StaleData { candle_date: Option<NaiveDate> },
}

/// Data pasar 1 putaran — dibaca sekali, dipakai semua bot.
struct Market {
    /// simbol -> (flex_param_id, candle lama -> baru)
    coins: HashMap<String, (Uuid, Vec<Candle>)>,
    fear_greed: Option<f64>,
    market_prices: HashMap<String, Decimal>,
    usdt_idr_rate: Decimal,
}

#[derive(Default)]
struct BotStats {
    filled: usize,
    failed: usize,
    opened: usize,
    closed: usize,
}

fn to_decimal(value: f64) -> Decimal {
    Decimal::from_f64(value).unwrap_or_default()
}

fn to_f64(value: Decimal) -> f64 {
    value.to_f64().unwrap_or_default()
}

/// Jalankan semua bot strategi aktif yang belum diproses hari ini (UTC).
pub async fn run_strategy_bots(pool: &PgPool) -> Result<StrategyBotRun, AppError> {
    let today = Utc::now().date_naive();
    let yesterday = today - Duration::days(1);

    let bots = {
        let mut tx = begin(pool).await?;
        let bots = find_bots_to_run(&mut tx, today).await?;
        commit(tx).await?;
        bots
    };
    if bots.is_empty() {
        return Ok(StrategyBotRun::Done(StrategyBotSummary::default()));
    }

    let daily = load_daily_market(pool, CANDLE_HISTORY).await?;
    if daily.candle_date() != Some(yesterday) {
        return Ok(StrategyBotRun::StaleData { candle_date: daily.candle_date() });
    }
    let market_prices = binance::get_all_prices().await?;
    let usdt_idr_rate = market_prices
        .get(USDT_IDR_SYMBOL)
        .copied()
        .filter(|r| !r.is_zero())
        .ok_or_else(|| AppError::Internal("Kurs USDTIDR tidak tersedia".to_string()))?;
    let market = Arc::new(Market {
        coins: daily
            .fresh_coins()
            .map(|(coin, history)| (coin.value_param.clone(), (coin.id, to_indicator_candles(history))))
            .collect(),
        fear_greed: recent_fear_greed_values(pool, 7).await?.last().copied(),
        market_prices,
        usdt_idr_rate,
    });

    let mut scripts: HashMap<Uuid, Arc<CompiledScript>> = HashMap::new();
    let mut summary = StrategyBotSummary::default();
    for bot in &bots {
        match process_bot(pool, bot, &market, &mut scripts, today).await {
            Ok(Some(stats)) => {
                summary.processed += 1;
                summary.orders_filled += stats.filled;
                summary.orders_failed += stats.failed;
                summary.positions_opened += stats.opened;
                summary.positions_closed += stats.closed;
            }
            Ok(None) => summary.skipped += 1,
            Err(err) => {
                summary.errors += 1;
                tracing::error!("Bot strategi {} gagal: {err:?}", bot.id);
            }
        }
    }
    Ok(StrategyBotRun::Done(summary))
}

/// Skor coin cakupan bot + coin yang sedang dipegang (dari candle close kemarin).
async fn scores(
    market: &Arc<Market>,
    script: Arc<CompiledScript>,
    symbols: Vec<String>,
    input_values: Value,
) -> Result<HashMap<String, ScriptOutput>, AppError> {
    let market = Arc::clone(market);
    tokio::task::spawn_blocking(move || {
        symbols
            .into_iter()
            .filter_map(|symbol| {
                let (_, candles) = market.coins.get(&symbol)?;
                let data = CoinData { symbol: &symbol, candles };
                let output = script.run(&data, &input_values, market.fear_greed).ok()?;
                Some((symbol, output))
            })
            .collect()
    })
    .await
    .map_err(|_| AppError::Internal("Gagal menjalankan script bot".to_string()))
}

/// Keputusan 1 hari: coin yang dijual & dibeli (urut skor tertinggi).
struct Plan {
    sell: Vec<String>,
    buy: Vec<(String, ScriptOutput)>,
}

fn plan(bot: &BotConfig, held: &HashSet<String>, scores: &HashMap<String, ScriptOutput>) -> Plan {
    let (entry, exit) = (to_f64(bot.entry_score), to_f64(bot.exit_score));
    let in_scope = |symbol: &String| bot.scope().is_none_or(|s| s.contains(symbol));
    let mut sell: Vec<String> = held
        .iter()
        .filter(|s| !in_scope(s) || scores.get(*s).is_some_and(|o| o.score <= exit))
        .cloned()
        .collect();
    sell.sort();
    let mut buy: Vec<(String, ScriptOutput)> = scores
        .iter()
        .filter(|(s, o)| o.score >= entry && !held.contains(*s) && in_scope(s))
        .map(|(s, o)| (s.clone(), o.clone()))
        .collect();
    buy.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
    let slots = (bot.max_positions.max(1) as usize).saturating_sub(held.len() - sell.len());
    buy.truncate(slots);
    Plan { sell, buy }
}

/// Proses 1 bot. `Ok(None)` = dilewati (strategi tidak bisa dipakai lagi).
async fn process_bot(
    pool: &PgPool,
    bot: &StrategySubscription,
    market: &Arc<Market>,
    scripts: &mut HashMap<Uuid, Arc<CompiledScript>>,
    today: NaiveDate,
) -> Result<Option<BotStats>, AppError> {
    let (user, version, positions) = {
        let mut tx = begin(pool).await?;
        let user = find_user_by_id(&mut tx, bot.user_id).await?.ok_or_else(|| AppError::NotFound("User tidak ditemukan".to_string()))?;
        if let Err(err) = usable_strategy_in_tx(&mut tx, bot.strategy_id, bot.user_id).await {
            // Strategi diarsipkan/dihapus/hak pakai habis: tidak trading, posisi lama dibiarkan.
            tracing::warn!("Bot strategi {} ({}) dilewati: {err:?}", bot.id, user.email);
            mark_bot_run(&mut tx, bot.id, today).await?;
            commit(tx).await?;
            return Ok(None);
        }
        let version = find_version_by_id(&mut tx, bot.strategy_version_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Versi strategi tidak ditemukan".to_string()))?;
        let positions = find_open_positions_of_subscription(&mut tx, bot.id).await?;
        commit(tx).await?;
        (user, version, positions)
    };

    let script = match scripts.get(&version.id) {
        Some(script) => Arc::clone(script),
        None => {
            let script = Arc::new(strategy_engine::compile(&version.source_code).map_err(AppError::BadRequest)?);
            scripts.insert(version.id, Arc::clone(&script));
            script
        }
    };
    let config = BotConfig::of(&version);
    let held: HashSet<String> = positions.iter().map(|p| p.symbol.clone()).collect();
    let mut symbols: Vec<String> = match config.scope() {
        Some(scope) => scope.to_vec(),
        None => market.coins.keys().cloned().collect(),
    };
    symbols.extend(held.iter().cloned());
    symbols.sort();
    symbols.dedup();
    let scores = scores(market, script, symbols, bot.input_values.clone()).await?;
    let plan = plan(&config, &held, &scores);
    let percent = bot.capital_percent.unwrap_or(Decimal::ONE_HUNDRED);

    let context = BotContext { pool, bot, user: &user, version: &version, market, today };
    let stats = match bot.account_mode.as_deref() {
        Some("LIVE") => context.run_live(positions, plan, percent).await?,
        _ => context.run_demo(positions, plan, percent).await?,
    };
    tracing::info!(
        "Bot strategi {} ({} {}): {} order, {} posisi baru, {} ditutup",
        bot.id,
        user.email,
        bot.account_mode.as_deref().unwrap_or("DEMO"),
        stats.filled,
        stats.opened,
        stats.closed
    );
    Ok(Some(stats))
}

struct BotContext<'a> {
    pool: &'a PgPool,
    bot: &'a StrategySubscription,
    user: &'a User,
    version: &'a StrategyVersion,
    market: &'a Arc<Market>,
    today: NaiveDate,
}

impl BotContext<'_> {
    fn account_mode(&self) -> &str {
        self.bot.account_mode.as_deref().unwrap_or("DEMO")
    }

    /// Baris posisi baru milik bot ini.
    #[allow(clippy::too_many_arguments)]
    fn new_row(
        &self,
        symbol: &str,
        quantity: Decimal,
        cost: Decimal,
        price: Decimal,
        fee: Decimal,
        account_capital: Decimal,
        order: Option<(String, Value)>,
    ) -> Option<NewTradePosition> {
        let flex_param_id = self.market.coins.get(symbol).map(|(id, _)| *id)?;
        let (entry_order_id, entry_order_response) = match order {
            Some((id, raw)) => (Some(id), Some(raw)),
            None => (None, None),
        };
        Some(NewTradePosition {
            user_id: self.user.id,
            user_email: self.user.email.clone(),
            flex_param_id,
            symbol: symbol.to_string(),
            source: SOURCE.to_string(),
            strategy: Some(STRATEGY_LABEL.to_string()),
            rule_type: RULE_TYPE.to_string(),
            rule_id: Some(self.version.id),
            strategy_subscription_id: Some(self.bot.id),
            opened_at: Utc::now(),
            closed_at: None,
            holding_days: 0,
            currency: self.user.preferred_currency.clone(),
            usdt_idr_rate: self.market.usdt_idr_rate,
            quantity: quantity.round_dp(8),
            entry_value: cost.round_dp(8),
            exit_value: None,
            entry_price: price.round_dp(8),
            exit_price: None,
            fee_amount: fee.round_dp(8),
            entry_order_id,
            exit_order_id: None,
            entry_order_response,
            exit_order_response: None,
            pnl_amount: Decimal::ZERO,
            pnl_percent: Decimal::ZERO,
            status: "OPEN".to_string(),
            result: None,
            account_capital: account_capital.round_dp(8),
            account_mode: self.account_mode().to_string(),
            created_by: SYSTEM_ACTOR.to_string(),
        })
    }

    /// DEMO: simulasi harga pasar, fee 0,1% (BUY dari coin, SELL dari USDT). 1 transaksi DB.
    async fn run_demo(&self, mut positions: Vec<TradePosition>, plan: Plan, percent: Decimal) -> Result<BotStats, AppError> {
        let prices = &self.market.market_prices;
        let price_of = |symbol: &str, fallback: Decimal| prices.get(symbol).copied().unwrap_or(fallback);
        let holdings_value: Decimal = positions.iter().map(|p| p.quantity * price_of(&p.symbol, p.entry_price)).sum();
        let account_value = self.user.demo_balance + holdings_value;
        let capital = robot_capital(self.user.demo_balance, holdings_value, percent);
        let mut cash = capital.cash;
        let mut stats = BotStats::default();
        let now = Utc::now();

        let symbols: Vec<String> = plan.buy.iter().map(|(s, _)| s.clone()).collect();
        let rules = if symbols.is_empty() { HashMap::new() } else { binance::get_trading_rules(&symbols).await? };

        let mut tx = begin(self.pool).await?;
        let mut new_rows = Vec::new();
        let mut proceeds_total = Decimal::ZERO;
        for symbol in &plan.sell {
            let quantity: Decimal = positions.iter().filter(|p| &p.symbol == symbol).map(|p| p.quantity).sum();
            let price = price_of(symbol, Decimal::ZERO);
            if quantity.is_zero() || price.is_zero() {
                continue;
            }
            let gross = quantity * price;
            let fee = gross * fee_rate();
            let fill = SellFill {
                symbol: symbol.clone(),
                quantity,
                net_total: gross - fee,
                fee_total: fee,
                price,
                at: now,
                order_id: None,
                order_response: None,
            };
            stats.closed += close_positions_fifo(&mut tx, &mut positions, &fill, &mut new_rows).await?;
            cash += fill.net_total;
            proceeds_total += fill.net_total;
            stats.filled += 1;
        }
        stats.closed += new_rows.len();

        let mut spent_total = Decimal::ZERO;
        for (symbol, output) in &plan.buy {
            let (Some(&price), Some(rule)) = (prices.get(symbol), rules.get(symbol)) else { continue };
            let size = output.size_percent.map(to_decimal).unwrap_or(self.version.position_percent);
            let amount = (capital.equity * size / Decimal::ONE_HUNDRED).min(cash);
            let quantity = floor_to_step(amount / price, rule.step_size);
            let cost = quantity * price;
            if quantity.is_zero() || cost > cash || rule.market_violation(quantity, cost).is_some() {
                continue;
            }
            let fee_quantity = quantity * fee_rate();
            if let Some(row) = self.new_row(symbol, quantity - fee_quantity, cost, price, fee_quantity * price, account_value, None) {
                cash -= cost;
                spent_total += cost;
                new_rows.push(row);
                stats.opened += 1;
                stats.filled += 1;
            }
        }

        create_trade_positions_bulk(&mut tx, &new_rows).await?;
        // Saldo demo berubah sebesar hasil jual − biaya beli (aman walau saldo berubah karena
        // transaksi manual di antaranya).
        let delta = (proceeds_total - spent_total).round_dp(8);
        if delta > Decimal::ZERO {
            credit_demo_balance(&mut tx, self.user.id, delta).await?;
        } else if delta < Decimal::ZERO && !debit_demo_balance(&mut tx, self.user.id, -delta).await? {
            return Err(AppError::BadRequest("Saldo demo tidak cukup".to_string()));
        }
        mark_bot_run(&mut tx, self.bot.id, self.today).await?;
        commit(tx).await?;
        Ok(stats)
    }

    /// LIVE: order MARKET asli. Tanggal proses dicatat dulu (order maks 1x per hari), tiap order
    /// dicatat di transaksinya sendiri; order yang gagal dilewati.
    async fn run_live(&self, mut positions: Vec<TradePosition>, plan: Plan, percent: Decimal) -> Result<BotStats, AppError> {
        let (Some(key), Some(secret)) = (&self.user.api_key, &self.user.api_secret) else {
            return Err(AppError::BadRequest("API key/secret belum diisi".to_string()));
        };
        let (api_key, api_secret) = (decrypt(key)?, decrypt(secret)?);
        let free: HashMap<String, Decimal> = binance::get_account_balances(&api_key, &api_secret)
            .await?
            .into_iter()
            .map(|b| (b.asset, b.free))
            .collect();
        let prices = binance::get_account_prices().await?;
        let free_of = |asset: &str| free.get(asset).copied().unwrap_or_default();

        // Coin milik bot, tidak lebih dari yang benar-benar ada di Binance.
        let mut holdings: HashMap<String, Decimal> = HashMap::new();
        for position in &positions {
            *holdings.entry(position.symbol.clone()).or_default() += position.quantity;
        }
        for (symbol, quantity) in holdings.iter_mut() {
            *quantity = (*quantity).min(free_of(base_asset(symbol)));
        }
        let holdings_value: Decimal = holdings.iter().map(|(s, q)| *q * prices.get(s).copied().unwrap_or_default()).sum();
        let free_usdt = free_of(QUOTE_ASSET);
        let account_value = free_usdt + holdings_value;
        let capital = robot_capital(free_usdt, holdings_value, percent);
        let mut cash = capital.cash;

        let mut symbols: Vec<String> = plan.sell.clone();
        symbols.extend(plan.buy.iter().map(|(s, _)| s.clone()));
        let rules = if symbols.is_empty() { HashMap::new() } else { binance::get_account_trading_rules(&symbols).await? };

        // Catat tanggal proses SEBELUM order dikirim — order uang asli maksimal 1x per hari.
        {
            let mut tx = begin(self.pool).await?;
            mark_bot_run(&mut tx, self.bot.id, self.today).await?;
            commit(tx).await?;
        }

        let mut stats = BotStats::default();
        for symbol in &plan.sell {
            let (Some(rule), Some(&price)) = (rules.get(symbol), prices.get(symbol)) else { continue };
            let held = holdings.get(symbol).copied().unwrap_or_default();
            match self.live_sell(symbol, held, price, *rule, &prices, &mut positions).await {
                Ok(Some((net, closed))) => {
                    cash += net;
                    stats.filled += 1;
                    stats.closed += closed;
                }
                Ok(None) => {}
                Err(err) => {
                    stats.failed += 1;
                    tracing::error!("Bot strategi {} SELL {symbol} gagal: {err:?}", self.bot.id);
                }
            }
        }
        for (symbol, output) in &plan.buy {
            let Some(rule) = rules.get(symbol) else { continue };
            let size = output.size_percent.map(to_decimal).unwrap_or(self.version.position_percent);
            let spend = (capital.equity * size / Decimal::ONE_HUNDRED)
                .min(cash)
                .round_dp_with_strategy(2, RoundingStrategy::ToZero);
            let price = prices.get(symbol).copied().unwrap_or_default();
            if price.is_zero() || rule.market_violation(floor_to_step(spend / price, rule.step_size), spend).is_some() {
                continue;
            }
            match self.live_buy(symbol, spend, account_value, &prices).await {
                Ok(spent) => {
                    cash -= spent;
                    stats.filled += 1;
                    stats.opened += 1;
                }
                Err(err) => {
                    stats.failed += 1;
                    tracing::error!("Bot strategi {} BUY {symbol} gagal: {err:?}", self.bot.id);
                }
            }
        }
        Ok(stats)
    }

    /// Jual seluruh coin bot (dibulatkan ke stepSize). Balik (USDT bersih, posisi ditutup).
    async fn live_sell(
        &self,
        symbol: &str,
        held: Decimal,
        price: Decimal,
        rule: TradingRules,
        prices: &HashMap<String, Decimal>,
        positions: &mut [TradePosition],
    ) -> Result<Option<(Decimal, usize)>, AppError> {
        let quantity = floor_to_step(held, rule.step_size);
        if rule.market_violation(quantity, quantity * price).is_some() {
            return Ok(None);
        }
        let (api_key, api_secret) = self.credentials()?;
        let (result, raw) = binance::place_market_order(&api_key, &api_secret, symbol, "SELL", OrderAmount::Base(quantity)).await?;
        let avg_price = average_price(&result);
        let net_total = result.quote_qty - commission_in(&result.fills, QUOTE_ASSET);
        let fill = SellFill {
            symbol: symbol.to_string(),
            quantity: result.executed_qty,
            net_total,
            fee_total: fee_in_usdt(&result.fills, symbol, avg_price, prices),
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
            Ok(closed) => Ok(Some((net_total, closed))),
            Err(err) => {
                self.log_unrecorded(symbol, &raw, &err);
                Ok(Some((net_total, 0)))
            }
        }
    }

    /// Beli senilai `spend` USDT (`quoteOrderQty`). Balik USDT yang terpakai.
    async fn live_buy(
        &self,
        symbol: &str,
        spend: Decimal,
        account_value: Decimal,
        prices: &HashMap<String, Decimal>,
    ) -> Result<Decimal, AppError> {
        let (api_key, api_secret) = self.credentials()?;
        let (result, raw) = binance::place_market_order(&api_key, &api_secret, symbol, "BUY", OrderAmount::Quote(spend)).await?;
        let price = average_price(&result);
        let quantity = result.executed_qty - commission_in(&result.fills, base_asset(symbol));
        let fee = fee_in_usdt(&result.fills, symbol, price, prices);
        let Some(row) = self.new_row(
            symbol,
            quantity,
            result.quote_qty,
            price,
            fee,
            account_value,
            Some((result.order_id.to_string(), raw.clone())),
        ) else {
            self.log_unrecorded(symbol, &raw, &AppError::NotFound("coin tidak ada di daftar".to_string()));
            return Ok(result.quote_qty);
        };
        let recorded = async {
            let mut tx = begin(self.pool).await?;
            create_trade_positions_bulk(&mut tx, std::slice::from_ref(&row)).await?;
            commit(tx).await
        }
        .await;
        if let Err(err) = recorded {
            self.log_unrecorded(symbol, &raw, &err);
        }
        Ok(result.quote_qty)
    }

    fn credentials(&self) -> Result<(String, String), AppError> {
        let (Some(key), Some(secret)) = (&self.user.api_key, &self.user.api_secret) else {
            return Err(AppError::BadRequest("API key/secret belum diisi".to_string()));
        };
        Ok((decrypt(key)?, decrypt(secret)?))
    }

    /// Order sudah terisi di Binance tapi gagal dicatat — jangan hilang: tulis lengkap ke log.
    fn log_unrecorded(&self, symbol: &str, raw: &Value, err: &AppError) {
        tracing::error!(
            "Bot strategi {} ({}): order {symbol} SUDAH TERISI di Binance tapi gagal dicatat ({err:?}) — cocokkan manual: {raw}",
            self.bot.id,
            self.user.email
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn output(score: f64) -> ScriptOutput {
        ScriptOutput { score, reason: String::new(), size_percent: None }
    }

    fn config(scope: &str, symbols: &[&str], max: i16) -> BotConfig {
        BotConfig {
            entry_score: d("70"),
            exit_score: d("30"),
            position_percent: d("20"),
            max_positions: max,
            coin_scope: scope.to_string(),
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn jual_skor_rendah_beli_skor_tinggi_dalam_batas_posisi() {
        let held: HashSet<String> = ["BTCUSDT".to_string(), "ETHUSDT".to_string()].into();
        let scores: HashMap<String, ScriptOutput> = [
            ("BTCUSDT".to_string(), output(20.0)), // dijual
            ("ETHUSDT".to_string(), output(50.0)), // ditahan
            ("ADAUSDT".to_string(), output(95.0)),
            ("XRPUSDT".to_string(), output(80.0)),
            ("SOLUSDT".to_string(), output(75.0)),
        ]
        .into();
        let plan = plan(&config("ALL", &[], 3), &held, &scores);
        assert_eq!(plan.sell, vec!["BTCUSDT".to_string()]);
        // maks 3 posisi: ETH tetap + 2 baru, skor tertinggi dulu
        let bought: Vec<&str> = plan.buy.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(bought, vec!["ADAUSDT", "XRPUSDT"]);
    }

    #[test]
    fn coin_tertentu_saja_dan_jual_yang_keluar_cakupan() {
        let held: HashSet<String> = ["ADAUSDT".to_string()].into();
        let scores: HashMap<String, ScriptOutput> =
            [("BTCUSDT".to_string(), output(90.0)), ("ADAUSDT".to_string(), output(90.0))].into();
        let plan = plan(&config("SPECIFIC", &["BTCUSDT"], 5), &held, &scores);
        assert_eq!(plan.sell, vec!["ADAUSDT".to_string()], "ADA di luar cakupan versi -> dijual");
        assert_eq!(plan.buy.len(), 1);
        assert_eq!(plan.buy[0].0, "BTCUSDT");
    }
}
