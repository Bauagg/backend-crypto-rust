//! Uji keandalan backtest (anti overfitting) — apakah hasil bagus itu nyata, atau kebetulan /
//! terlalu pas dengan data masa lalu:
//! 1. out-of-sample: 70% awal vs 30% akhir periode. Strategi yang overfit bagus di awal lalu
//!    jeblok di data yang belum "dilihat";
//! 2. Monte Carlo: return harian diambil ulang acak per blok 10 hari (block bootstrap) 1.000x —
//!    sebaran return & drawdown yang mungkin terjadi + peluang rugi;
//! 3. banding acak: tiap trade diganti beli coin & tanggal acak dengan lama pegang yang sama, 300x —
//!    apakah strategi lebih baik dari kebetulan (p-value);
//! 4. sensitivitas: skor masuk/keluar & setelan script digeser sedikit. Strategi yang overfit
//!    hancur kalau angkanya diubah sedikit.
//!
//! Keempatnya digabung jadi skor risiko overfitting 0–100 (RENDAH / SEDANG / TINGGI). Acak memakai
//! seed tetap: backtest yang sama selalu memberi hasil yang sama.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::{json, Value};

use super::backtest::{
    compute_signals, daily_returns, round, simulate, BacktestConfig, BacktestTrade, Market, SignalMatrix, Simulation,
};
use super::inputs::InputSpec;
use super::CompiledScript;

const SEED: u64 = 20_261_027;
/// Porsi akhir periode yang dijadikan data "ujian".
const OUT_OF_SAMPLE_SHARE: f64 = 0.3;
const MONTE_CARLO_RUNS: usize = 1000;
const BLOCK_DAYS: usize = 10;
const RANDOM_RUNS: usize = 300;
/// Setelan script yang diuji sensitivitasnya (tiap setelan = 2 kali menjalankan ulang script).
const MAX_INPUTS_TESTED: usize = 2;
const INPUT_SHIFT: f64 = 0.2;
const MIN_CLOSED_TRADES: usize = 20;
/// Bobot maksimal tiap uji di skor risiko.
const PART: f64 = 25.0;

/// Hasil 1 uji: data untuk FE, sumbangan risiko (0–`PART`, `None` = tidak dinilai), dan kalimat.
struct Check {
    json: Value,
    risk: Option<f64>,
    strengths: Vec<String>,
    weaknesses: Vec<String>,
}

impl Check {
    fn skipped(reason: impl Into<String>) -> Self {
        Self { json: json!({ "available": false, "reason": reason.into() }), risk: None, strengths: vec![], weaknesses: vec![] }
    }
}

pub(super) struct Validation {
    pub json: Value,
    label: &'static str,
    strengths: Vec<String>,
    weaknesses: Vec<String>,
}

impl Validation {
    /// Tambahkan hasil uji ke label ketahanan & kelebihan/kelemahan. Risiko overfitting TINGGI
    /// menurunkan label KUAT jadi SEDANG.
    pub fn apply(&self, robustness: &mut Value, analysis: &mut Value) {
        robustness["overfitting_risk"] = json!(self.label);
        if self.label == "TINGGI" && robustness["label"] == "KUAT" {
            robustness["label"] = json!("SEDANG");
        }
        for (key, items) in [("strengths", &self.strengths), ("weaknesses", &self.weaknesses)] {
            if let Some(list) = analysis[key].as_array_mut() {
                list.extend(items.iter().map(|s| json!(s)));
            }
        }
    }
}

fn fraction(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Persentil dari data yang sudah urut (p 0–1).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn sort(values: &mut [f64]) {
    values.sort_by(f64::total_cmp);
}

fn closed_trades(result: &Simulation) -> Vec<&BacktestTrade> {
    result.trades.iter().filter(|t| t.exit_date.is_some()).collect()
}

pub(super) fn validate(
    script: &CompiledScript,
    market: &Market,
    signals: &SignalMatrix,
    result: &Simulation,
    input_values: &Value,
    input_specs: &[InputSpec],
    cfg: &BacktestConfig,
) -> Validation {
    let mut rng = StdRng::seed_from_u64(SEED);
    let checks = [
        ("out_of_sample", out_of_sample(result)),
        ("monte_carlo", monte_carlo(result, &mut rng)),
        ("random_baseline", random_baseline(market, result, cfg, &mut rng)),
        ("sensitivity", sensitivity(script, market, signals, result, input_values, input_specs, cfg)),
    ];

    let base_return = result.total_return(cfg.initial_capital);
    let closed = closed_trades(result).len();
    let risks: Vec<f64> = checks.iter().filter_map(|(_, c)| c.risk).collect();
    let mut strengths: Vec<String> = checks.iter().flat_map(|(_, c)| c.strengths.clone()).collect();
    let mut weaknesses: Vec<String> = checks.iter().flat_map(|(_, c)| c.weaknesses.clone()).collect();

    let (score, label) = if base_return <= 0.0 {
        // Strategi yang tidak profit tidak "terlalu pas" — memang belum bagus.
        (None, "TIDAK_RELEVAN")
    } else if risks.is_empty() {
        (None, "DATA_KURANG")
    } else {
        let mut score = risks.iter().sum::<f64>() / (risks.len() as f64 * PART) * 100.0;
        // Trade sedikit = uji kurang meyakinkan (kalimatnya sudah ada di kelemahan statistik).
        if closed < MIN_CLOSED_TRADES {
            score = (score + 15.0).min(100.0);
        }
        let label = if score < 30.0 {
            "RENDAH"
        } else if score < 60.0 {
            "SEDANG"
        } else {
            "TINGGI"
        };
        (Some(round(score, 0)), label)
    };
    match label {
        "RENDAH" => strengths.push("Risiko overfitting rendah — hasil backtest kemungkinan besar bukan kebetulan".to_string()),
        "TINGGI" => weaknesses.push(
            "Risiko overfitting TINGGI — hasil bagus kemungkinan hanya cocok dengan data masa lalu".to_string(),
        ),
        _ => {}
    }

    let mut json = json!({
        "overfitting_risk": label,
        "risk_score": score,
        "explanation": "Skor 0–100 dari 4 uji (masing-masing 25): out-of-sample, Monte Carlo, banding acak, sensitivitas setelan. < 30 RENDAH, 30–59 SEDANG, ≥ 60 TINGGI. Trade selesai < 20 menambah 15.",
        "few_trades": closed < MIN_CLOSED_TRADES,
    });
    for (key, check) in checks {
        json[key] = check.json;
    }
    Validation { json, label, strengths, weaknesses }
}

/// 70% awal ("latihan") vs 30% akhir ("ujian"): return per tahun & win rate masing-masing.
fn out_of_sample(result: &Simulation) -> Check {
    let curve = &result.equity_curve;
    let days = curve.len().saturating_sub(1);
    if days < 60 {
        return Check::skipped("Periode terlalu pendek untuk uji out-of-sample (minimal 60 hari)");
    }
    let split = (days as f64 * (1.0 - OUT_OF_SAMPLE_SHARE)).round() as usize;
    let split_date = curve[split].date;
    let segment = |a: usize, b: usize, in_sample: bool| {
        let total = if curve[a].equity > 0.0 { (curve[b].equity / curve[a].equity - 1.0) * 100.0 } else { 0.0 };
        let annual = total / (b - a).max(1) as f64 * 365.0;
        let trades: Vec<&BacktestTrade> = result
            .trades
            .iter()
            .filter(|t| t.exit_date.is_some() && (t.entry_date < split_date) == in_sample)
            .collect();
        let wins = trades.iter().filter(|t| t.pnl_amount > 0.0).count();
        let win_rate = if trades.is_empty() { 0.0 } else { wins as f64 / trades.len() as f64 * 100.0 };
        let json = json!({
            "from": curve[a].date,
            "to": curve[b].date,
            "return_percent": round(total, 2),
            "annual_return_percent": round(annual, 2),
            "closed_trades": trades.len(),
            "win_rate_percent": round(win_rate, 2),
        });
        (json, annual)
    };
    let (in_json, in_annual) = segment(0, split, true);
    let (out_json, out_annual) = segment(split, days, false);

    let mut check = Check { json: Value::Null, risk: None, strengths: vec![], weaknesses: vec![] };
    let ratio = (in_annual > 0.0).then(|| out_annual / in_annual);
    if let Some(ratio) = ratio {
        check.risk = Some(fraction((0.7 - ratio) / 0.7) * PART);
        if ratio >= 0.7 {
            check.strengths.push(format!(
                "Tetap bagus di 30% periode terakhir (data uji): {out_annual:+.1}%/tahun vs {in_annual:+.1}%/tahun sebelumnya"
            ));
        } else if ratio < 0.3 {
            check.weaknesses.push(format!(
                "Hasil jauh menurun di 30% periode terakhir ({out_annual:+.1}%/tahun vs {in_annual:+.1}%/tahun) — tanda overfitting"
            ));
        }
    }
    check.json = json!({
        "available": true,
        "split_date": split_date,
        "in_sample": in_json,
        "out_of_sample": out_json,
        // return/tahun data uji ÷ data latihan: ≥ 0,7 bagus, < 0,3 tanda overfitting
        "consistency_ratio": ratio.map(|r| round(r, 2)),
    });
    check
}

/// Block bootstrap return harian: sebaran total return & max drawdown + peluang rugi.
fn monte_carlo(result: &Simulation, rng: &mut StdRng) -> Check {
    let returns = daily_returns(&result.equity_curve);
    let n = returns.len();
    if n < 30 {
        return Check::skipped("Periode terlalu pendek untuk Monte Carlo (minimal 30 hari)");
    }
    let mut totals = Vec::with_capacity(MONTE_CARLO_RUNS);
    let mut drawdowns = Vec::with_capacity(MONTE_CARLO_RUNS);
    for _ in 0..MONTE_CARLO_RUNS {
        let (mut equity, mut peak, mut worst) = (1.0f64, 1.0f64, 0.0f64);
        let mut taken = 0;
        while taken < n {
            let start = rng.gen_range(0..n);
            for k in 0..BLOCK_DAYS.min(n - taken) {
                equity *= 1.0 + returns[(start + k) % n];
                peak = peak.max(equity);
                worst = worst.min((equity - peak) / peak * 100.0);
            }
            taken += BLOCK_DAYS.min(n - taken);
        }
        totals.push((equity - 1.0) * 100.0);
        drawdowns.push(worst);
    }
    sort(&mut totals);
    sort(&mut drawdowns);
    let loss_chance = totals.iter().filter(|r| **r < 0.0).count() as f64 / MONTE_CARLO_RUNS as f64 * 100.0;
    let bad_drawdown = percentile(&drawdowns, 0.05);

    let mut check = Check {
        json: json!({
            "available": true,
            "simulations": MONTE_CARLO_RUNS,
            "block_days": BLOCK_DAYS,
            "return_percent": {
                "worst_5": round(percentile(&totals, 0.05), 2),
                "median": round(percentile(&totals, 0.5), 2),
                "best_95": round(percentile(&totals, 0.95), 2),
            },
            "max_drawdown_percent": {
                "worst_5": round(bad_drawdown, 2),
                "median": round(percentile(&drawdowns, 0.5), 2),
                "best_95": round(percentile(&drawdowns, 0.95), 2),
            },
            "probability_of_loss_percent": round(loss_chance, 1),
        }),
        risk: Some(fraction(loss_chance / 50.0) * PART),
        strengths: vec![],
        weaknesses: vec![],
    };
    if loss_chance <= 10.0 {
        check.strengths.push(format!("Monte Carlo {MONTE_CARLO_RUNS} skenario: peluang rugi hanya {loss_chance:.0}%"));
    } else if loss_chance >= 30.0 {
        check.weaknesses.push(format!(
            "Monte Carlo {MONTE_CARLO_RUNS} skenario: peluang rugi {loss_chance:.0}% — hasil bagus bisa jadi kebetulan urutan pasar"
        ));
    }
    if bad_drawdown < -40.0 {
        check.weaknesses.push(format!("Di skenario buruk (5% terburuk) modal bisa turun sampai {bad_drawdown:.0}%"));
    }
    check
}

/// Rata-rata return per trade strategi vs beli coin & tanggal acak dengan lama pegang yang sama.
fn random_baseline(market: &Market, result: &Simulation, cfg: &BacktestConfig, rng: &mut StdRng) -> Check {
    let closed = closed_trades(result);
    if closed.len() < 5 {
        return Check::skipped("Trade terlalu sedikit untuk dibandingkan dengan beli acak (minimal 5)");
    }
    let real = closed.iter().map(|t| t.pnl_percent).sum::<f64>() / closed.len() as f64;
    let sim = &market.sim;
    let coins = market.histories.len();
    let keep = (1.0 - cfg.fee_rate).powi(2);
    let mut means = Vec::with_capacity(RANDOM_RUNS);
    for _ in 0..RANDOM_RUNS {
        let (mut sum, mut count) = (0.0, 0usize);
        for trade in &closed {
            let hold = trade.holding_days.max(1) as usize;
            if hold + 1 >= sim.len() {
                continue;
            }
            for _attempt in 0..10 {
                let s = rng.gen_range(0..coins);
                let d = rng.gen_range(1..sim.len() - hold);
                let (Some((_, entry)), Some((_, exit))) = (market.candle_on(s, sim[d]), market.candle_on(s, sim[d + hold])) else {
                    continue;
                };
                if entry.open > 0.0 {
                    sum += (keep * exit.open / entry.open - 1.0) * 100.0;
                    count += 1;
                    break;
                }
            }
        }
        if count > 0 {
            means.push(sum / count as f64);
        }
    }
    if means.is_empty() {
        return Check::skipped("Data harga tidak cukup untuk banding acak");
    }
    let as_good = means.iter().filter(|m| **m >= real).count();
    let p_value = (as_good + 1) as f64 / (means.len() + 1) as f64;
    sort(&mut means);

    let mut check = Check {
        json: json!({
            "available": true,
            "runs": means.len(),
            "strategy_avg_trade_percent": round(real, 2),
            "random_avg_trade_percent": round(percentile(&means, 0.5), 2),
            "random_best_5_percent": round(percentile(&means, 0.95), 2),
            // peluang strategi acak menyamai/mengalahkan strategi ini; ≤ 0,05 = keunggulan nyata
            "p_value": round(p_value, 3),
            "beats_random_percent": round((1.0 - p_value) * 100.0, 1),
        }),
        risk: Some(fraction((p_value - 0.05) / 0.45) * PART),
        strengths: vec![],
        weaknesses: vec![],
    };
    if p_value <= 0.05 {
        check.strengths.push(format!(
            "Lebih baik dari {:.0}% percobaan beli coin acak (p-value {p_value:.3}) — bukan sekadar kebetulan",
            (1.0 - p_value) * 100.0
        ));
    } else if p_value > 0.2 {
        check.weaknesses.push(format!(
            "Tidak jauh beda dari beli coin acak (p-value {p_value:.2}) — keunggulan strategi belum terbukti"
        ));
    }
    check
}

fn variation(parameter: &str, value: f64, result: &Simulation, cfg: &BacktestConfig) -> (Value, f64) {
    let total = result.total_return(cfg.initial_capital);
    let json = json!({
        "parameter": parameter,
        "value": round(value, 4),
        "total_return_percent": round(total, 2),
        "max_drawdown_percent": round(result.max_drawdown(), 2),
        "trades": result.trades.len(),
    });
    (json, total)
}

/// Geser skor masuk/keluar & setelan script sedikit, lihat apakah hasil tetap bagus.
fn sensitivity(
    script: &CompiledScript,
    market: &Market,
    signals: &SignalMatrix,
    result: &Simulation,
    input_values: &Value,
    input_specs: &[InputSpec],
    cfg: &BacktestConfig,
) -> Check {
    let base = result.total_return(cfg.initial_capital);
    let mut rows: Vec<(Value, f64)> = Vec::new();

    // Skor masuk/keluar: skor script sama, cukup ulang simulasi order.
    for shift in [-10.0, -5.0, 5.0, 10.0] {
        let entry = cfg.entry_score + shift;
        if (1.0..=100.0).contains(&entry) && cfg.exit_score < entry {
            let changed = BacktestConfig { entry_score: entry, ..cfg.clone() };
            rows.push(variation("entry_score", entry, &simulate(market, signals, &changed), cfg));
        }
    }
    for shift in [-10.0, 10.0] {
        let exit = cfg.exit_score + shift;
        if (0.0..cfg.entry_score).contains(&exit) {
            let changed = BacktestConfig { exit_score: exit, ..cfg.clone() };
            rows.push(variation("exit_score", exit, &simulate(market, signals, &changed), cfg));
        }
    }
    // Setelan script ±20%: skor berubah, script dijalankan ulang.
    for spec in input_specs.iter().take(MAX_INPUTS_TESTED) {
        let current = input_values.get(&spec.name).and_then(Value::as_f64).unwrap_or(spec.default);
        for factor in [1.0 - INPUT_SHIFT, 1.0 + INPUT_SHIFT] {
            let mut value = current * factor;
            if spec.integer {
                value = value.round();
            }
            value = value.clamp(spec.min, spec.max);
            if (value - current).abs() < 1e-9 {
                continue;
            }
            let mut values = input_values.clone();
            values[spec.name.as_str()] = if spec.integer { json!(value as i64) } else { json!(value) };
            let changed = compute_signals(script, market, &values, cfg);
            rows.push(variation(&format!("inputs.{}", spec.name), value, &simulate(market, &changed, cfg), cfg));
        }
    }
    if rows.is_empty() {
        return Check::skipped("Tidak ada setelan yang bisa digeser");
    }

    let mut returns: Vec<f64> = rows.iter().map(|(_, r)| *r).collect();
    sort(&mut returns);
    let profitable = returns.iter().filter(|r| **r > 0.0).count();
    let share = profitable as f64 / returns.len() as f64;
    let median = percentile(&returns, 0.5);
    let mut check = Check {
        json: json!({
            "available": true,
            "base_return_percent": round(base, 2),
            "variations": rows.into_iter().map(|(json, _)| json).collect::<Vec<_>>(),
            "profitable_variations_percent": round(share * 100.0, 1),
            "median_return_percent": round(median, 2),
        }),
        risk: None,
        strengths: vec![],
        weaknesses: vec![],
    };
    if base > 0.0 {
        let kept = median / base;
        check.risk = Some(((1.0 - share) * 0.5 + fraction(1.0 - kept) * 0.5) * PART);
        if share >= 0.8 && kept >= 0.5 {
            check.strengths.push(format!(
                "Stabil: {profitable} dari {} variasi setelan tetap profit",
                returns.len()
            ));
        }
    }
    if share < 0.5 {
        check.weaknesses.push(format!(
            "Rapuh: hanya {profitable} dari {} variasi setelan yang profit — hasil bergantung pada angka setelan tertentu",
            returns.len()
        ));
    }
    check
}
