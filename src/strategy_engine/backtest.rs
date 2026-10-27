//! Backtest: simulasi robot strategi di histori candle harian — aturan sama dengan bot sungguhan:
//! - sinyal dari candle CLOSE hari D (script hanya melihat candle s/d hari D), order dijalankan di
//!   harga OPEN hari D+1 — tidak pernah melihat masa depan;
//! - JUAL coin yang dipegang kalau skor <= exit_score, lalu BELI coin skor >= entry_score (skor
//!   tertinggi dulu) selama posisi < max_positions;
//! - ukuran posisi = persen modal (dari script `signal(.., .., persen)` atau `position_percent`) ×
//!   nilai akun hari itu, tidak lebih dari kas; order < 5 USDT dilewati (minimum Binance) — jadi
//!   modal kecil bisa memberi hasil berbeda dari modal besar;
//! - fee per transaksi `fee_rate` (BUY dipotong dari coin, SELL dari USDT — seperti robot demo).
//!
//! Skor script dihitung SEKALI per coin per hari (`compute_signals`), lalu simulasi order
//! (`simulate`) bisa diulang murah — dipakai uji sensitivitas skor masuk/keluar di `validation`.
//!
//! Hasil: daftar trade, kurva modal (+ drawdown & pembanding beli-tahan BTC), statistik,
//! uji ketahanan, kelebihan/kelemahan dalam bahasa sederhana, uji overfitting (`validation`), dan
//! analisa faktor yang paling memengaruhi untung/rugi (`insights`).

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use serde_json::{json, Value};

use super::inputs::InputSpec;
use super::{insights, validation};
use super::{CoinData, CompiledScript, ScriptOutput};
use crate::indicator::Candle;

/// Order di bawah ini dilewati (minimum notional Binance).
const MIN_ORDER_USDT: f64 = 5.0;
/// Periode ATR untuk volatilitas coin saat masuk (bahan `insights`).
const ATR_PERIOD: usize = 14;
/// Rentang perubahan BTC untuk kondisi pasar saat masuk (bahan `insights`).
const BTC_TREND_DAYS: usize = 30;

/// Histori 1 coin, lama -> baru; `dates[i]` = tanggal `candles[i]`.
pub struct SymbolHistory {
    pub symbol: String,
    pub dates: Vec<NaiveDate>,
    pub candles: Vec<Candle>,
}

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub entry_score: f64,
    pub exit_score: f64,
    /// Persen modal per coin kalau script tidak menentukan sendiri.
    pub position_percent: f64,
    pub max_positions: usize,
    pub initial_capital: f64,
    pub fee_rate: f64,
    /// Jumlah candle terakhir yang diberikan ke script tiap hari (cukup untuk indikator 200 hari).
    pub window: usize,
    /// Coin yang historinya kurang dari ini pada hari itu tidak dinilai.
    pub min_candles: usize,
    /// Lama simulasi (hari terakhir).
    pub period_days: usize,
}

/// Kondisi saat sinyal BELI — bahan analisa faktor (`insights`), tidak disimpan ke DB.
#[derive(Debug, Clone, Default)]
pub struct EntryContext {
    pub fear_greed: Option<f64>,
    /// Perubahan harga BTC 30 hari terakhir (%) pada hari sinyal.
    pub btc_change_30d: Option<f64>,
    /// ATR 14 hari ÷ close (%) coin pada hari sinyal.
    pub volatility_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacktestTrade {
    pub symbol: String,
    pub entry_date: NaiveDate,
    /// `None` = masih dipegang saat backtest berakhir (dinilai dengan close terakhir).
    pub exit_date: Option<NaiveDate>,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: f64,
    pub entry_value: f64,
    pub exit_value: f64,
    pub fee_amount: f64,
    pub pnl_amount: f64,
    pub pnl_percent: f64,
    pub holding_days: i64,
    pub entry_score: f64,
    pub exit_score: Option<f64>,
    pub entry_reason: String,
    pub exit_reason: Option<String>,
    #[serde(skip)]
    pub context: EntryContext,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    pub date: NaiveDate,
    pub equity: f64,
    /// Penurunan dari puncak modal sebelumnya (≤ 0).
    pub drawdown_percent: f64,
    /// Nilai kalau modal awal dipakai beli-tahan coin pembanding (BTC).
    pub btc_equity: Option<f64>,
}

pub struct BacktestOutput {
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub trades: Vec<BacktestTrade>,
    pub equity_curve: Vec<EquityPoint>,
    pub stats: Value,
    pub robustness: Value,
    pub analysis: Value,
    /// Uji overfitting: out-of-sample, Monte Carlo, banding acak, sensitivitas setelan.
    pub validation: Value,
    /// Faktor yang paling memengaruhi untung/rugi trade + saran.
    pub insights: Value,
}

struct Position {
    quantity: f64,
    entry_value: f64,
    entry_price: f64,
    entry_fee: f64,
    entry_date: NaiveDate,
    entry_score: f64,
    entry_reason: String,
    context: EntryContext,
}

pub(super) fn round(value: f64, decimals: i32) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let factor = 10f64.powi(decimals);
    (value * factor).round() / factor
}

/// Histori yang sudah diindeks per tanggal — dibangun sekali, dipakai simulasi berulang.
pub(super) struct Market<'a> {
    pub histories: &'a [SymbolHistory],
    index: Vec<HashMap<NaiveDate, usize>>,
    /// Hari simulasi; hari pertama = modal awal, sinyal hari `sim[d]` dieksekusi di `sim[d + 1]`.
    pub sim: Vec<NaiveDate>,
    benchmark: Option<(&'a SymbolHistory, HashMap<NaiveDate, usize>)>,
    fear_greed: &'a HashMap<NaiveDate, f64>,
}

impl<'a> Market<'a> {
    fn new(
        histories: &'a [SymbolHistory],
        benchmark: Option<&'a SymbolHistory>,
        fear_greed: &'a HashMap<NaiveDate, f64>,
        period_days: usize,
    ) -> Result<Self, String> {
        let mut calendar: Vec<NaiveDate> =
            histories.iter().flat_map(|h| h.dates.iter().copied()).collect::<HashSet<_>>().into_iter().collect();
        calendar.sort();
        if calendar.len() < 30 {
            return Err("Histori candle belum cukup untuk backtest".to_string());
        }
        let index_of = |h: &SymbolHistory| h.dates.iter().enumerate().map(|(i, d)| (*d, i)).collect();
        Ok(Self {
            histories,
            index: histories.iter().map(index_of).collect(),
            sim: calendar[calendar.len().saturating_sub(period_days + 1)..].to_vec(),
            benchmark: benchmark.map(|b| (b, index_of(b))),
            fear_greed,
        })
    }

    pub fn candle_on(&self, s: usize, date: NaiveDate) -> Option<(usize, Candle)> {
        self.index[s].get(&date).map(|&i| (i, self.histories[s].candles[i]))
    }

    fn benchmark_close(&self, date: NaiveDate) -> Option<Candle> {
        let (b, index) = self.benchmark.as_ref()?;
        Some(b.candles[*index.get(&date)?])
    }

    /// Perubahan close BTC `days` candle terakhir (%) pada `date`.
    fn btc_change(&self, date: NaiveDate, days: usize) -> Option<f64> {
        let (b, index) = self.benchmark.as_ref()?;
        let i = *index.get(&date)?;
        let then = b.candles[i.checked_sub(days)?].close;
        (then > 0.0).then(|| (b.candles[i].close / then - 1.0) * 100.0)
    }
}

/// ATR `period` ÷ close (%) pada candle ke-`i`.
fn atr_percent(candles: &[Candle], i: usize, period: usize) -> Option<f64> {
    if i < period {
        return None;
    }
    let true_range: f64 = (i + 1 - period..=i)
        .map(|k| {
            let (c, prev) = (candles[k], candles[k - 1].close);
            (c.high - c.low).max((c.high - prev).abs()).max((c.low - prev).abs())
        })
        .sum();
    let close = candles[i].close;
    (close > 0.0).then(|| true_range / period as f64 / close * 100.0)
}

/// Skor script tiap coin per hari sinyal: `signals[d]` = sinyal hari `sim[d]`.
pub(super) type SignalMatrix = Vec<HashMap<usize, ScriptOutput>>;

/// Jalankan script ke semua coin tiap hari simulasi. Coin yang script-nya error pada hari tertentu
/// dianggap tidak bersinyal hari itu.
pub(super) fn compute_signals(script: &CompiledScript, market: &Market, input_values: &Value, cfg: &BacktestConfig) -> SignalMatrix {
    let days = market.sim.len().saturating_sub(1);
    (0..days)
        .map(|d| {
            let day = market.sim[d];
            let mut signals = HashMap::new();
            for (s, history) in market.histories.iter().enumerate() {
                let Some((i, _)) = market.candle_on(s, day) else { continue };
                if i + 1 < cfg.min_candles {
                    continue;
                }
                let start = (i + 1).saturating_sub(cfg.window);
                let data = CoinData { symbol: &history.symbol, candles: &history.candles[start..=i] };
                if let Ok(output) = script.run(&data, input_values, market.fear_greed.get(&day).copied()) {
                    signals.insert(s, output);
                }
            }
            signals
        })
        .collect()
}

pub(super) struct Simulation {
    pub trades: Vec<BacktestTrade>,
    pub equity_curve: Vec<EquityPoint>,
    /// % hari memegang minimal 1 coin.
    pub exposure: f64,
}

impl Simulation {
    pub fn total_return(&self, initial_capital: f64) -> f64 {
        let end = self.equity_curve.last().map(|p| p.equity).unwrap_or(initial_capital);
        (end / initial_capital - 1.0) * 100.0
    }

    pub fn max_drawdown(&self) -> f64 {
        self.equity_curve.iter().map(|p| p.drawdown_percent).fold(0.0, f64::min)
    }
}

/// Simulasi order dari skor yang sudah dihitung — murah, bisa diulang dengan aturan berbeda.
pub(super) fn simulate(market: &Market, signals: &SignalMatrix, cfg: &BacktestConfig) -> Simulation {
    let histories = market.histories;
    let sim = &market.sim;
    let mut cash = cfg.initial_capital;
    let mut positions: HashMap<usize, Position> = HashMap::new();
    let mut last_close: HashMap<usize, f64> = HashMap::new();
    let mut trades = Vec::new();
    let mut equity_curve = vec![EquityPoint { date: sim[0], equity: cash, drawdown_percent: 0.0, btc_equity: None }];
    let mut peak = cash;
    let mut exposed_days = 0usize;
    let mut benchmark_qty: Option<f64> = None;

    for (d, today_signals) in signals.iter().enumerate() {
        let (day, next) = (sim[d], sim[d + 1]);
        for s in 0..histories.len() {
            if let Some((_, candle)) = market.candle_on(s, day) {
                last_close.insert(s, candle.close);
            }
        }
        let equity_today = cash + positions.iter().map(|(s, p)| p.quantity * last_close.get(s).copied().unwrap_or(p.entry_price)).sum::<f64>();

        // 1. JUAL di open besok.
        let mut to_sell: Vec<usize> = positions
            .keys()
            .copied()
            .filter(|s| today_signals.get(s).is_some_and(|o| o.score <= cfg.exit_score) && market.candle_on(*s, next).is_some())
            .collect();
        to_sell.sort_unstable();
        for s in to_sell {
            let position = positions.remove(&s).expect("posisi ada");
            let (_, candle) = market.candle_on(s, next).expect("dicek di atas");
            let gross = position.quantity * candle.open;
            let exit_fee = gross * cfg.fee_rate;
            let exit_value = gross - exit_fee;
            cash += exit_value;
            let signal = today_signals.get(&s);
            let pnl = exit_value - position.entry_value;
            trades.push(BacktestTrade {
                symbol: histories[s].symbol.clone(),
                entry_date: position.entry_date,
                exit_date: Some(next),
                entry_price: position.entry_price,
                exit_price: candle.open,
                quantity: position.quantity,
                entry_value: position.entry_value,
                exit_value,
                fee_amount: position.entry_fee + exit_fee,
                pnl_amount: pnl,
                pnl_percent: pnl / position.entry_value * 100.0,
                holding_days: (next - position.entry_date).num_days(),
                entry_score: position.entry_score,
                exit_score: signal.map(|o| o.score),
                entry_reason: position.entry_reason,
                exit_reason: signal.map(|o| o.reason.clone()),
                context: position.context,
            });
        }

        // 2. BELI di open besok: skor tertinggi dulu.
        let mut candidates: Vec<(usize, &ScriptOutput)> = today_signals
            .iter()
            .filter(|(s, o)| o.score >= cfg.entry_score && !positions.contains_key(s) && market.candle_on(**s, next).is_some())
            .map(|(s, o)| (*s, o))
            .collect();
        candidates.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| histories[a.0].symbol.cmp(&histories[b.0].symbol)));
        for (s, output) in candidates {
            if positions.len() >= cfg.max_positions {
                break;
            }
            let size = output.size_percent.unwrap_or(cfg.position_percent);
            let amount = (equity_today * size / 100.0).min(cash);
            if amount < MIN_ORDER_USDT {
                continue;
            }
            let (_, candle) = market.candle_on(s, next).expect("dicek di atas");
            if candle.open <= 0.0 {
                continue;
            }
            let signal_index = market.candle_on(s, day).map(|(i, _)| i);
            let context = EntryContext {
                fear_greed: market.fear_greed.get(&day).copied(),
                btc_change_30d: market.btc_change(day, BTC_TREND_DAYS),
                volatility_percent: signal_index.and_then(|i| atr_percent(&histories[s].candles, i, ATR_PERIOD)),
            };
            let entry_fee = amount * cfg.fee_rate;
            cash -= amount;
            positions.insert(s, Position {
                quantity: (amount - entry_fee) / candle.open,
                entry_value: amount,
                entry_price: candle.open,
                entry_fee,
                entry_date: next,
                entry_score: output.score,
                entry_reason: output.reason.clone(),
                context,
            });
        }

        // 3. Nilai akun di close besok.
        for s in positions.keys() {
            if let Some((_, candle)) = market.candle_on(*s, next) {
                last_close.insert(*s, candle.close);
            }
        }
        let equity = cash + positions.iter().map(|(s, p)| p.quantity * last_close.get(s).copied().unwrap_or(p.entry_price)).sum::<f64>();
        if !positions.is_empty() {
            exposed_days += 1;
        }
        peak = peak.max(equity);
        let btc_equity = market.benchmark_close(next).map(|candle| {
            let qty = *benchmark_qty.get_or_insert(cfg.initial_capital / candle.open);
            qty * candle.close
        });
        equity_curve.push(EquityPoint {
            date: next,
            equity,
            drawdown_percent: if peak > 0.0 { (equity - peak) / peak * 100.0 } else { 0.0 },
            btc_equity,
        });
    }

    // Posisi yang masih dipegang dinilai dengan close terakhir (+ fee jual hipotetis).
    let end = *sim.last().expect("sim tidak kosong");
    let mut held: Vec<(usize, Position)> = positions.into_iter().collect();
    held.sort_by_key(|(s, _)| *s);
    for (s, position) in held {
        let price = last_close.get(&s).copied().unwrap_or(position.entry_price);
        let gross = position.quantity * price;
        let exit_fee = gross * cfg.fee_rate;
        let exit_value = gross - exit_fee;
        let pnl = exit_value - position.entry_value;
        trades.push(BacktestTrade {
            symbol: histories[s].symbol.clone(),
            entry_date: position.entry_date,
            exit_date: None,
            entry_price: position.entry_price,
            exit_price: price,
            quantity: position.quantity,
            entry_value: position.entry_value,
            exit_value,
            fee_amount: position.entry_fee + exit_fee,
            pnl_amount: pnl,
            pnl_percent: pnl / position.entry_value * 100.0,
            holding_days: (end - position.entry_date).num_days(),
            entry_score: position.entry_score,
            exit_score: None,
            entry_reason: position.entry_reason,
            exit_reason: None,
            context: position.context,
        });
    }
    trades.sort_by_key(|t| (t.entry_date, t.symbol.clone()));

    Simulation { trades, equity_curve, exposure: exposed_days as f64 / (sim.len() - 1).max(1) as f64 * 100.0 }
}

/// Jalankan backtest ke coin `histories` (yang boleh ditradingkan). `benchmark` = coin pembanding
/// beli-tahan (BTC) — hanya untuk kurva pembanding & kondisi pasar, tidak ikut ditradingkan.
/// `input_specs` = deklarasi setelan script (untuk uji sensitivitas). Error = data tidak cukup.
pub fn run_backtest(
    script: &CompiledScript,
    histories: &[SymbolHistory],
    benchmark: Option<&SymbolHistory>,
    input_values: &Value,
    input_specs: &[InputSpec],
    fear_greed: &HashMap<NaiveDate, f64>,
    cfg: &BacktestConfig,
) -> Result<BacktestOutput, String> {
    let market = Market::new(histories, benchmark, fear_greed, cfg.period_days)?;
    let signals = compute_signals(script, &market, input_values, cfg);
    let result = simulate(&market, &signals, cfg);

    let (stats, mut robustness, mut analysis) = summarize(&result.trades, &result.equity_curve, cfg, result.exposure);
    let checked = validation::validate(script, &market, &signals, &result, input_values, input_specs, cfg);
    checked.apply(&mut robustness, &mut analysis);
    let insights = insights::analyze(&result.trades, cfg);

    Ok(BacktestOutput {
        period_start: market.sim[0],
        period_end: *market.sim.last().expect("sim tidak kosong"),
        trades: result.trades,
        equity_curve: result.equity_curve,
        stats,
        robustness,
        analysis,
        validation: checked.json,
        insights,
    })
}

/// Statistik, uji ketahanan, & kelebihan/kelemahan dari hasil simulasi.
fn summarize(trades: &[BacktestTrade], curve: &[EquityPoint], cfg: &BacktestConfig, exposure: f64) -> (Value, Value, Value) {
    let start = cfg.initial_capital;
    let end_equity = curve.last().map(|p| p.equity).unwrap_or(start);
    let total_return = (end_equity / start - 1.0) * 100.0;
    let days = (curve.len().saturating_sub(1)).max(1) as f64;
    let cagr = ((end_equity / start).powf(365.0 / days) - 1.0) * 100.0;

    let max_drawdown = curve.iter().map(|p| p.drawdown_percent).fold(0.0, f64::min);
    // Drawdown terlama: hari dari puncak sampai pulih (atau sampai akhir).
    let (mut longest_dd, mut current_dd) = (0usize, 0usize);
    for point in curve {
        if point.drawdown_percent < 0.0 {
            current_dd += 1;
            longest_dd = longest_dd.max(current_dd);
        } else {
            current_dd = 0;
        }
    }

    let daily = daily_returns(curve);
    let mean = daily.iter().sum::<f64>() / daily.len().max(1) as f64;
    let std = (daily.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily.len().max(1) as f64).sqrt();
    let downside = (daily.iter().filter(|r| **r < 0.0).map(|r| r.powi(2)).sum::<f64>() / daily.len().max(1) as f64).sqrt();
    let sharpe = if std > 0.0 { mean / std * 365f64.sqrt() } else { 0.0 };
    let sortino = if downside > 0.0 { mean / downside * 365f64.sqrt() } else { 0.0 };

    let closed: Vec<&BacktestTrade> = trades.iter().filter(|t| t.exit_date.is_some()).collect();
    let wins: Vec<&&BacktestTrade> = closed.iter().filter(|t| t.pnl_amount > 0.0).collect();
    let losses: Vec<&&BacktestTrade> = closed.iter().filter(|t| t.pnl_amount <= 0.0).collect();
    let gross_win: f64 = wins.iter().map(|t| t.pnl_amount).sum();
    let gross_loss: f64 = -losses.iter().map(|t| t.pnl_amount).sum::<f64>();
    let win_rate = if closed.is_empty() { 0.0 } else { wins.len() as f64 / closed.len() as f64 * 100.0 };
    let profit_factor = if gross_loss > 0.0 { gross_win / gross_loss } else if gross_win > 0.0 { 99.0 } else { 0.0 };
    let avg = |items: &[&&BacktestTrade]| if items.is_empty() { 0.0 } else { items.iter().map(|t| t.pnl_percent).sum::<f64>() / items.len() as f64 };
    let avg_holding = if trades.is_empty() { 0.0 } else { trades.iter().map(|t| t.holding_days as f64).sum::<f64>() / trades.len() as f64 };
    let best = trades.iter().map(|t| t.pnl_percent).fold(f64::NAN, f64::max);
    let worst = trades.iter().map(|t| t.pnl_percent).fold(f64::NAN, f64::min);
    let mut max_consecutive_losses = 0;
    let mut streak = 0;
    for trade in &closed {
        if trade.pnl_amount <= 0.0 {
            streak += 1;
            max_consecutive_losses = max_consecutive_losses.max(streak);
        } else {
            streak = 0;
        }
    }

    // Return per bulan.
    let mut month_end: BTreeMap<(i32, u32), f64> = BTreeMap::new();
    for point in curve {
        month_end.insert((point.date.year(), point.date.month()), point.equity);
    }
    let mut monthly = Vec::new();
    let mut previous = start;
    for ((year, month), equity) in &month_end {
        monthly.push(json!({"month": format!("{year}-{month:02}"), "return_percent": round((equity / previous - 1.0) * 100.0, 2)}));
        previous = *equity;
    }
    let month_returns: Vec<f64> = monthly.iter().filter_map(|m| m["return_percent"].as_f64()).collect();
    let profitable_months = if month_returns.is_empty() { 0.0 } else { month_returns.iter().filter(|r| **r > 0.0).count() as f64 / month_returns.len() as f64 * 100.0 };

    let btc_start = curve.iter().find_map(|p| p.btc_equity).unwrap_or(start);
    let btc_end = curve.iter().rev().find_map(|p| p.btc_equity);
    let btc_return = btc_end.map(|e| (e / btc_start - 1.0) * 100.0);
    let btc_drawdown = {
        let mut peak = f64::MIN;
        curve.iter().filter_map(|p| p.btc_equity).fold(0.0, |worst: f64, e| {
            peak = peak.max(e);
            worst.min((e - peak) / peak * 100.0)
        })
    };

    let mid = curve.len() / 2;
    let half = |a: usize, b: usize| if curve[a].equity > 0.0 { (curve[b].equity / curve[a].equity - 1.0) * 100.0 } else { 0.0 };
    let (first_half, second_half) = (half(0, mid), half(mid, curve.len() - 1));

    let stats = json!({
        "initial_capital": round(start, 2),
        "final_equity": round(end_equity, 2),
        "total_return_percent": round(total_return, 2),
        "cagr_percent": round(cagr, 2),
        "max_drawdown_percent": round(max_drawdown, 2),
        "sharpe_ratio": round(sharpe, 2),
        "sortino_ratio": round(sortino, 2),
        "volatility_percent": round(std * 365f64.sqrt() * 100.0, 2),
        "trades": trades.len(),
        "closed_trades": closed.len(),
        "win_rate_percent": round(win_rate, 2),
        "profit_factor": round(profit_factor, 2),
        "avg_win_percent": round(avg(&wins), 2),
        "avg_loss_percent": round(avg(&losses), 2),
        "best_trade_percent": round(best, 2),
        "worst_trade_percent": round(worst, 2),
        "avg_holding_days": round(avg_holding, 1),
        "exposure_percent": round(exposure, 2),
        "btc_return_percent": btc_return.map(|r| round(r, 2)),
        "btc_max_drawdown_percent": round(btc_drawdown, 2),
        "beats_btc": btc_return.map(|r| total_return > r),
        "monthly_returns": monthly,
    });

    // Label ketahanan: KUAT / SEDANG / LEMAH + alasannya (bisa diturunkan uji overfitting).
    let enough_trades = closed.len() >= 20;
    let consistent = first_half > 0.0 && second_half > 0.0;
    let label = if total_return <= 0.0 || max_drawdown < -50.0 || closed.len() < 5 {
        "LEMAH"
    } else if consistent && enough_trades && max_drawdown > -30.0 && profit_factor >= 1.3 {
        "KUAT"
    } else {
        "SEDANG"
    };
    let robustness = json!({
        "label": label,
        "first_half_return_percent": round(first_half, 2),
        "second_half_return_percent": round(second_half, 2),
        "consistent_halves": consistent,
        "profitable_months_percent": round(profitable_months, 2),
        "longest_drawdown_days": longest_dd,
        "max_consecutive_losses": max_consecutive_losses,
        "enough_trades": enough_trades,
    });

    let mut strengths = Vec::new();
    let mut weaknesses = Vec::new();
    match btc_return {
        Some(btc) if total_return > btc => strengths.push(format!("Lebih baik dari beli-tahan BTC ({:+.1}% vs {:+.1}%)", total_return, btc)),
        Some(btc) => weaknesses.push(format!("Kalah dari beli-tahan BTC ({:+.1}% vs {:+.1}%)", total_return, btc)),
        None => {}
    }
    if max_drawdown > -20.0 {
        strengths.push(format!("Penurunan modal terdalam kecil ({:.1}%)", max_drawdown));
    } else if max_drawdown < -35.0 {
        weaknesses.push(format!("Penurunan modal terdalam besar ({:.1}%) — siapkan mental saat rugi beruntun", max_drawdown));
    }
    if btc_drawdown < 0.0 && max_drawdown > btc_drawdown {
        strengths.push(format!("Drawdown lebih kecil dari BTC ({:.1}% vs {:.1}%)", max_drawdown, btc_drawdown));
    }
    if win_rate >= 55.0 {
        strengths.push(format!("Lebih sering untung: {:.0}% trade profit", win_rate));
    } else if !closed.is_empty() && win_rate < 40.0 {
        weaknesses.push(format!("Lebih sering rugi: hanya {:.0}% trade profit", win_rate));
    }
    if profit_factor >= 1.5 {
        strengths.push(format!("Untung rata-rata jauh lebih besar dari rugi (profit factor {:.2})", profit_factor));
    } else if profit_factor < 1.0 && !closed.is_empty() {
        weaknesses.push(format!("Total rugi lebih besar dari total untung (profit factor {:.2})", profit_factor));
    }
    if consistent {
        strengths.push(format!("Konsisten: paruh awal {:+.1}% dan paruh akhir {:+.1}%", first_half, second_half));
    } else {
        weaknesses.push(format!("Tidak konsisten: paruh awal {:+.1}%, paruh akhir {:+.1}% — bisa jadi hanya cocok di kondisi pasar tertentu", first_half, second_half));
    }
    if !enough_trades {
        weaknesses.push(format!("Jumlah trade sedikit ({}) — hasil belum bisa diandalkan", closed.len()));
    }
    if exposure < 20.0 {
        weaknesses.push(format!("Modal lebih banyak menganggur ({:.0}% waktu memegang coin)", exposure));
    }
    let analysis = json!({ "strengths": strengths, "weaknesses": weaknesses });

    (stats, robustness, analysis)
}

/// Return harian kurva modal (pecahan, mis. 0.01 = +1%).
pub(super) fn daily_returns(curve: &[EquityPoint]) -> Vec<f64> {
    curve.windows(2).map(|w| if w[0].equity > 0.0 { w[1].equity / w[0].equity - 1.0 } else { 0.0 }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy_engine::compile;

    fn history(symbol: &str, closes: &[f64]) -> SymbolHistory {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        SymbolHistory {
            symbol: symbol.to_string(),
            dates: (0..closes.len()).map(|i| start + chrono::Duration::days(i as i64)).collect(),
            candles: closes.iter().map(|c| Candle { open: *c, high: c * 1.01, low: c * 0.99, close: *c, volume: 1000.0 }).collect(),
        }
    }

    fn config() -> BacktestConfig {
        BacktestConfig {
            entry_score: 70.0,
            exit_score: 30.0,
            position_percent: 50.0,
            max_positions: 2,
            initial_capital: 1000.0,
            fee_rate: 0.001,
            window: 300,
            min_candles: 5,
            period_days: 365,
        }
    }

    #[test]
    fn beli_saat_naik_jual_saat_turun_tanpa_melihat_masa_depan() {
        // 40 hari naik lalu 40 hari turun. Script: skor 100 kalau close > close kemarin, 0 kalau turun.
        let closes: Vec<f64> = (0..40).map(|i| 100.0 + i as f64).chain((0..40).map(|i| 139.0 - i as f64)).collect();
        let histories = vec![history("BTCUSDT", &closes)];
        let script = compile("if last(close) > prev(close, 1) { signal(100, \"naik\") } else { signal(0, \"turun\") }").unwrap();
        let out = run_backtest(&script, &histories, histories.first(), &Value::Null, &[], &HashMap::new(), &config()).unwrap();

        assert_eq!(out.trades.len(), 1, "1x masuk saat naik, 1x keluar saat mulai turun");
        let trade = &out.trades[0];
        assert!(trade.exit_date.is_some());
        // dibeli di open hari setelah sinyal (bukan di harga sinyal) — 50% modal
        assert!((trade.entry_value - 500.0).abs() < 1e-6);
        assert!(trade.pnl_amount > 0.0, "naik 100 -> 139 lalu jual saat mulai turun = untung");
        assert!(out.stats["total_return_percent"].as_f64().unwrap() > 0.0);
        assert_eq!(out.stats["beats_btc"], json!(true), "BTC beli-tahan balik ke awal, strategi untung");
        assert!(out.equity_curve.iter().all(|p| p.drawdown_percent <= 0.0));
        // Masuk di hari ke-5: histori BTC belum 30 hari, jadi kondisi pasar belum bisa dinilai.
        assert!(trade.context.btc_change_30d.is_none());
        assert!(out.validation["overfitting_risk"].is_string());
        assert_eq!(out.insights["available"], json!(false), "1 trade terlalu sedikit untuk analisa faktor");
    }

    #[test]
    fn validasi_dan_insights_terisi_untuk_banyak_trade() {
        // Harga naik-turun bergelombang 200 hari, 3 coin: banyak trade masuk-keluar.
        let wave = |phase: f64| -> Vec<f64> {
            (0..200).map(|i| 100.0 + 15.0 * ((i as f64 / 6.0) + phase).sin() + i as f64 * 0.1).collect()
        };
        let histories = vec![history("BTCUSDT", &wave(0.0)), history("ETHUSDT", &wave(1.0)), history("SOLUSDT", &wave(2.0))];
        let script = compile(
            "let r = last(rsi(close, inputs.period)); if r < 45 { signal(90, \"murah\") } else if r > 60 { signal(10, \"mahal\") } else { signal(50, \"tahan\") }",
        )
        .unwrap();
        let specs = vec![InputSpec { name: "period".to_string(), label: String::new(), default: 7.0, min: 2.0, max: 30.0, integer: true }];
        let values = json!({ "period": 7 });
        let mut cfg = config();
        cfg.position_percent = 30.0;
        cfg.max_positions = 3;
        let out = run_backtest(&script, &histories, histories.first(), &values, &specs, &HashMap::new(), &cfg).unwrap();

        assert!(out.trades.len() >= 10, "trade: {}", out.trades.len());
        let v = &out.validation;
        assert_eq!(v["out_of_sample"]["available"], json!(true));
        assert_eq!(v["monte_carlo"]["simulations"], json!(1000));
        assert!(v["random_baseline"]["p_value"].is_number());
        let variations = v["sensitivity"]["variations"].as_array().unwrap();
        assert!(variations.iter().any(|x| x["parameter"] == "inputs.period"), "setelan script ikut diuji");
        assert!(variations.iter().any(|x| x["parameter"] == "entry_score"));
        assert_eq!(out.insights["available"], json!(true));
        assert!(!out.insights["factors"].as_array().unwrap().is_empty());
        assert!(!out.insights["recommendations"].as_array().unwrap().is_empty());

        // Seed tetap: backtest yang sama memberi hasil validasi yang sama.
        let again = run_backtest(&script, &histories, histories.first(), &values, &specs, &HashMap::new(), &cfg).unwrap();
        assert_eq!(out.validation, again.validation);
    }

    #[test]
    fn ukuran_posisi_dari_script_dan_minimum_order() {
        let closes: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let histories = vec![history("BTCUSDT", &closes), history("ETHUSDT", &closes)];
        // BTC 10% modal, ETH 0,4% (= 4 USDT < minimum 5) -> ETH dilewati
        let script = compile("if symbol == \"BTCUSDT\" { signal(90, \"a\", 10) } else { signal(90, \"b\", 0.4 + 1) }").unwrap();
        let mut cfg = config();
        cfg.initial_capital = 300.0;
        let out = run_backtest(&script, &histories, None, &Value::Null, &[], &HashMap::new(), &cfg).unwrap();
        let btc = out.trades.iter().find(|t| t.symbol == "BTCUSDT").unwrap();
        assert!((btc.entry_value - 30.0).abs() < 1e-6, "10% dari 300");
        assert!(out.trades.iter().all(|t| t.symbol != "ETHUSDT"), "1,4% dari 300 = 4,2 USDT < 5 dilewati");
    }

    #[test]
    fn modal_kecil_bisa_melewatkan_order() {
        // 3% per coin: modal 1000 -> 30 USDT (jalan), modal 100 -> 3 USDT (< 5, dilewati semua).
        let closes: Vec<f64> = (0..40).map(|i| 100.0 + i as f64).collect();
        let histories = vec![history("BTCUSDT", &closes)];
        let script = compile("signal(90, \"x\", 3)").unwrap();
        let mut cfg = config();
        let big = run_backtest(&script, &histories, None, &Value::Null, &[], &HashMap::new(), &cfg).unwrap();
        cfg.initial_capital = 100.0;
        let small = run_backtest(&script, &histories, None, &Value::Null, &[], &HashMap::new(), &cfg).unwrap();
        assert_eq!(big.trades.len(), 1);
        assert!(small.trades.is_empty());
    }
}
