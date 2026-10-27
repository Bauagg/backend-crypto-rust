//! Analisa faktor — seperti *feature importance* di ML: trade hasil backtest dikelompokkan per
//! faktor (coin, kondisi pasar BTC, Fear & Greed, skor masuk, lama pegang, volatilitas, hari),
//! lalu dihitung seberapa besar faktor itu menjelaskan beda untung/rugi antar trade.
//!
//! Ukurannya epsilon-squared: porsi variasi return per trade yang dijelaskan pengelompokan, sudah
//! dikoreksi jumlah kelompok — supaya faktor dengan banyak kelompok (mis. 40 coin) tidak terlihat
//! penting hanya karena kebetulan. 0% = tidak berpengaruh. Hanya kelompok dengan minimal 3 trade
//! yang dihitung.
//!
//! Lalu diuji signifikansinya dengan uji permutasi: return trade diacak ulang antar kelompok 500x;
//! p-value = peluang pengaruh sebesar itu muncul karena kebetulan. Kesimpulan & saran hanya dari
//! faktor yang signifikan (p < 0,1) — dengan trade sedikit, wajar kalau belum ada yang signifikan.

use std::collections::HashMap;

use chrono::Datelike;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use serde_json::{json, Value};

use super::backtest::{round, BacktestConfig, BacktestTrade};

const MIN_TRADES: usize = 10;
/// Kelompok dengan trade kurang dari ini tidak dipakai untuk kesimpulan.
const MIN_GROUP_TRADES: usize = 3;
const PERMUTATIONS: usize = 500;
const SIGNIFICANT_P: f64 = 0.1;
const SEED: u64 = 20_261_028;

/// Kelompok 1 trade: (nama, urutan tampil). `None` = data faktor tidak ada.
type Grouping = fn(&BacktestTrade) -> Option<(String, i64)>;

struct Factor {
    key: &'static str,
    label: &'static str,
    group: Grouping,
}

const WEEKDAYS: [&str; 7] = ["Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu", "Minggu"];

const FACTORS: [Factor; 7] = [
    Factor { key: "coin", label: "Coin", group: |t| Some((t.symbol.clone(), 0)) },
    Factor {
        key: "market_regime",
        label: "Kondisi pasar (BTC 30 hari)",
        group: |t| {
            let change = t.context.btc_change_30d?;
            Some(if change > 5.0 {
                ("BTC naik (> +5%)".to_string(), 0)
            } else if change < -5.0 {
                ("BTC turun (< -5%)".to_string(), 2)
            } else {
                ("BTC sideways".to_string(), 1)
            })
        },
    },
    Factor {
        key: "fear_greed",
        label: "Fear & Greed saat masuk",
        group: |t| {
            let value = t.context.fear_greed?;
            Some(match value {
                v if v < 25.0 => ("Extreme Fear (0–24)".to_string(), 0),
                v if v < 45.0 => ("Fear (25–44)".to_string(), 1),
                v if v <= 55.0 => ("Neutral (45–55)".to_string(), 2),
                v if v <= 75.0 => ("Greed (56–75)".to_string(), 3),
                _ => ("Extreme Greed (76–100)".to_string(), 4),
            })
        },
    },
    Factor {
        key: "entry_score",
        label: "Skor saat masuk",
        group: |t| {
            let bucket = ((t.entry_score / 10.0).floor() * 10.0).min(90.0) as i64;
            let upper = if bucket >= 90 { 100 } else { bucket + 9 };
            Some((format!("{bucket}–{upper}"), bucket))
        },
    },
    Factor {
        key: "holding_days",
        label: "Lama pegang",
        group: |t| {
            Some(match t.holding_days {
                d if d <= 3 => ("1–3 hari".to_string(), 0),
                d if d <= 7 => ("4–7 hari".to_string(), 1),
                d if d <= 14 => ("8–14 hari".to_string(), 2),
                d if d <= 30 => ("15–30 hari".to_string(), 3),
                _ => ("> 30 hari".to_string(), 4),
            })
        },
    },
    Factor {
        key: "volatility",
        label: "Volatilitas coin (ATR 14 hari)",
        group: |t| {
            let atr = t.context.volatility_percent?;
            Some(if atr < 3.0 {
                ("Rendah (< 3%)".to_string(), 0)
            } else if atr <= 6.0 {
                ("Sedang (3–6%)".to_string(), 1)
            } else {
                ("Tinggi (> 6%)".to_string(), 2)
            })
        },
    },
    Factor {
        key: "weekday",
        label: "Hari masuk",
        group: |t| {
            let day = t.entry_date.weekday().num_days_from_monday() as usize;
            Some((WEEKDAYS[day].to_string(), day as i64))
        },
    },
];

struct Group {
    name: String,
    order: i64,
    trades: usize,
    win_rate: f64,
    avg: f64,
    total: f64,
}

impl Group {
    fn enough(&self) -> bool {
        self.trades >= MIN_GROUP_TRADES
    }
}

struct FactorResult {
    key: &'static str,
    label: &'static str,
    importance: f64,
    /// Uji permutasi; `None` = kurang dari 2 kelompok yang cukup datanya.
    p_value: Option<f64>,
    groups: Vec<Group>,
    finding: Option<String>,
}

impl FactorResult {
    fn significant(&self) -> bool {
        self.p_value.is_some_and(|p| p < SIGNIFICANT_P)
    }

    fn group(&self, order: i64) -> Option<&Group> {
        self.groups.iter().find(|g| g.order == order && g.enough())
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() { 0.0 } else { values.iter().sum::<f64>() / values.len() as f64 }
}

/// Epsilon-squared: porsi variasi `values` yang dijelaskan pengelompokan `groups`.
fn epsilon_squared(groups: &[Vec<f64>]) -> f64 {
    let all: Vec<f64> = groups.iter().flatten().copied().collect();
    let (n, k) = (all.len(), groups.len());
    let m = mean(&all);
    let ss_total: f64 = all.iter().map(|x| (x - m).powi(2)).sum();
    if ss_total <= 0.0 || n <= k {
        return 0.0;
    }
    let ss_between: f64 = groups.iter().map(|g| g.len() as f64 * (mean(g) - m).powi(2)).sum();
    let ms_within = (ss_total - ss_between) / (n - k) as f64;
    ((ss_between - (k - 1) as f64 * ms_within) / (ss_total + ms_within)).clamp(0.0, 1.0)
}

/// Pengaruh faktor (epsilon-squared, %) & p-value uji permutasi dari kelompok yang cukup datanya.
fn importance(groups: &[Vec<f64>], rng: &mut StdRng) -> (f64, Option<f64>) {
    if groups.len() < 2 {
        return (0.0, None);
    }
    let observed = epsilon_squared(groups);
    let mut pooled: Vec<f64> = groups.iter().flatten().copied().collect();
    let sizes: Vec<usize> = groups.iter().map(Vec::len).collect();
    let mut as_large = 0usize;
    for _ in 0..PERMUTATIONS {
        pooled.shuffle(rng);
        let mut start = 0;
        let shuffled: Vec<Vec<f64>> = sizes
            .iter()
            .map(|size| {
                let group = pooled[start..start + size].to_vec();
                start += size;
                group
            })
            .collect();
        if epsilon_squared(&shuffled) >= observed {
            as_large += 1;
        }
    }
    (observed * 100.0, Some((as_large + 1) as f64 / (PERMUTATIONS + 1) as f64))
}

fn evaluate(factor: &Factor, trades: &[BacktestTrade], rng: &mut StdRng) -> Option<FactorResult> {
    let mut buckets: HashMap<String, (i64, Vec<&BacktestTrade>)> = HashMap::new();
    for trade in trades {
        if let Some((name, order)) = (factor.group)(trade) {
            buckets.entry(name).or_insert_with(|| (order, Vec::new())).1.push(trade);
        }
    }
    let covered: usize = buckets.values().map(|(_, t)| t.len()).sum();
    if covered < MIN_TRADES || buckets.len() < 2 {
        return None;
    }
    // Urutan tetap (bukan urutan HashMap) supaya hasil acak dengan seed selalu sama.
    let mut reliable: Vec<(&String, &(i64, Vec<&BacktestTrade>))> =
        buckets.iter().filter(|(_, (_, t))| t.len() >= MIN_GROUP_TRADES).collect();
    reliable.sort_by(|a, b| a.0.cmp(b.0));
    let returns: Vec<Vec<f64>> = reliable.iter().map(|(_, (_, t))| t.iter().map(|x| x.pnl_percent).collect()).collect();
    let (importance, p_value) = importance(&returns, rng);

    let mut groups: Vec<Group> = buckets
        .into_iter()
        .map(|(name, (order, items))| {
            let pnl: Vec<f64> = items.iter().map(|t| t.pnl_percent).collect();
            Group {
                name,
                order,
                trades: items.len(),
                win_rate: items.iter().filter(|t| t.pnl_amount > 0.0).count() as f64 / items.len() as f64 * 100.0,
                avg: mean(&pnl),
                total: items.iter().map(|t| t.pnl_amount).sum(),
            }
        })
        .collect();
    groups.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| b.avg.total_cmp(&a.avg)));

    let reliable: Vec<&Group> = groups.iter().filter(|g| g.enough()).collect();
    let finding = match (
        reliable.iter().max_by(|a, b| a.avg.total_cmp(&b.avg)),
        reliable.iter().min_by(|a, b| a.avg.total_cmp(&b.avg)),
    ) {
        (Some(best), Some(worst)) if reliable.len() >= 2 => Some(format!(
            "{}: paling bagus di {} (rata-rata {:+.1}% per trade, {} trade), paling buruk di {} ({:+.1}%, {} trade)",
            factor.label, best.name, best.avg, best.trades, worst.name, worst.avg, worst.trades
        )),
        _ => None,
    };
    Some(FactorResult { key: factor.key, label: factor.label, importance, p_value, groups, finding })
}

/// Saran konkret dari faktor yang signifikan secara statistik.
fn recommend(factors: &[FactorResult], cfg: &BacktestConfig) -> Vec<String> {
    let find = |key: &str| factors.iter().find(|f| f.key == key && f.significant());
    let mut out = Vec::new();

    if let Some(score) = find("entry_score") {
        let best = score.groups.iter().filter(|g| g.enough()).max_by(|a, b| a.avg.total_cmp(&b.avg));
        if let Some(best) = best {
            let lower_loses = score.groups.iter().any(|g| g.enough() && g.order < best.order && g.avg < 0.0);
            if best.avg > 0.0 && best.order as f64 > cfg.entry_score && lower_loses {
                out.push(format!(
                    "Trade dengan skor masuk {} paling bagus (rata-rata {:+.1}%) sedangkan skor lebih rendah rugi — coba naikkan entry_score ke {}",
                    best.name, best.avg, best.order
                ));
            }
        }
    }
    if let Some(coin) = find("coin") {
        let mut losers: Vec<&Group> = coin.groups.iter().filter(|g| g.enough() && g.avg < 0.0 && g.total < 0.0).collect();
        losers.sort_by(|a, b| a.total.total_cmp(&b.total));
        if !losers.is_empty() {
            let names: Vec<&str> = losers.iter().take(3).map(|g| g.name.as_str()).collect();
            out.push(format!(
                "Coin yang paling sering rugi: {} — pertimbangkan coin_scope SPECIFIC tanpa coin ini",
                names.join(", ")
            ));
        }
    }
    if let Some(market) = find("market_regime")
        && let (Some(up), Some(down)) = (market.group(0), market.group(2))
        && down.avg < 0.0
        && up.avg > 0.0
    {
        out.push(format!(
            "Untung saat BTC naik ({:+.1}%) tapi rugi saat BTC turun ({:+.1}%) — tambahkan filter tren di script, mis. hanya beli kalau last(close) > last(ema(close, 200))",
            up.avg, down.avg
        ));
    }
    if let Some(fear) = find("fear_greed") {
        if let Some(greed) = fear.group(4).filter(|g| g.avg < 0.0) {
            out.push(format!(
                "Trade saat Extreme Greed rata-rata rugi ({:+.1}%) — tambahkan syarat fear_greed < 75 di script",
                greed.avg
            ));
        }
        if let Some(extreme_fear) = fear.group(0).filter(|g| g.avg > 0.0) {
            let best = fear.groups.iter().filter(|g| g.enough()).all(|g| g.avg <= extreme_fear.avg);
            if best {
                out.push(format!(
                    "Paling untung saat Extreme Fear ({:+.1}% per trade) — strategi cocok untuk membeli saat pasar panik",
                    extreme_fear.avg
                ));
            }
        }
    }
    if let Some(holding) = find("holding_days") {
        let long_loses = holding.groups.iter().any(|g| g.enough() && g.order >= 3 && g.avg < 0.0);
        let short_wins = holding.groups.iter().any(|g| g.enough() && g.order <= 1 && g.avg > 0.0);
        if long_loses && short_wins {
            out.push("Posisi yang dipegang lebih dari 14 hari cenderung rugi — naikkan exit_score supaya keluar lebih cepat".to_string());
        }
    }
    if let Some(volatility) = find("volatility")
        && let Some(high) = volatility.group(2).filter(|g| g.avg < 0.0)
    {
        out.push(format!(
            "Coin sangat volatil (ATR > 6%) rata-rata rugi ({:+.1}%) — saring di script, mis. last(atr(high, low, close, 14)) / last(close) < 0.06",
            high.avg
        ));
    }
    if out.is_empty() {
        out.push(if factors.iter().any(FactorResult::significant) {
            "Belum ada pola yang perlu diubah dari data trade ini".to_string()
        } else {
            "Belum ada faktor yang signifikan secara statistik — perlu lebih banyak trade untuk menyimpulkan pola".to_string()
        });
    }
    out
}

pub(super) fn analyze(trades: &[BacktestTrade], cfg: &BacktestConfig) -> Value {
    if trades.len() < MIN_TRADES {
        return json!({
            "available": false,
            "reason": format!("Butuh minimal {MIN_TRADES} trade untuk analisa faktor (ada {})", trades.len()),
        });
    }
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut factors: Vec<FactorResult> = FACTORS.iter().filter_map(|f| evaluate(f, trades, &mut rng)).collect();
    // Faktor signifikan dulu, lalu pengaruh terbesar.
    factors.sort_by(|a, b| b.significant().cmp(&a.significant()).then_with(|| b.importance.total_cmp(&a.importance)));
    let total_importance: f64 = factors.iter().filter(|f| f.significant()).map(|f| f.importance).sum();
    let recommendations = recommend(&factors, cfg);
    let pnl: Vec<f64> = trades.iter().map(|t| t.pnl_percent).collect();
    let wins = trades.iter().filter(|t| t.pnl_amount > 0.0).count();

    json!({
        "available": true,
        "trades_analyzed": trades.len(),
        "avg_pnl_percent": round(mean(&pnl), 2),
        "win_rate_percent": round(wins as f64 / trades.len() as f64 * 100.0, 2),
        "method": "Pengaruh = epsilon-squared: porsi beda untung/rugi antar trade yang dijelaskan faktor (0% = tidak berpengaruh), dari kelompok dengan minimal 3 trade. Signifikan = uji permutasi 500x, p-value < 0,1 (bukan kebetulan).",
        "top_factor": factors.iter().find(|f| f.significant() && f.importance > 0.0).map(|f| f.key),
        "factors": factors.iter().map(|f| json!({
            "key": f.key,
            "label": f.label,
            "importance_percent": round(f.importance, 2),
            "p_value": f.p_value.map(|p| round(p, 3)),
            "significant": f.significant(),
            // porsi pengaruh di antara faktor yang signifikan
            "share_percent": if f.significant() && total_importance > 0.0 { round(f.importance / total_importance * 100.0, 1) } else { 0.0 },
            "finding": f.finding,
            "groups": f.groups.iter().map(|g| json!({
                "name": g.name,
                "trades": g.trades,
                "win_rate_percent": round(g.win_rate, 2),
                "avg_pnl_percent": round(g.avg, 2),
                "total_pnl_amount": round(g.total, 2),
                "enough_data": g.enough(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "recommendations": recommendations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epsilon_squared_nol_kalau_kelompok_sama_dan_tinggi_kalau_beda_jelas() {
        let same = vec![vec![1.0, -1.0, 2.0, -2.0], vec![1.0, -1.0, 2.0, -2.0]];
        assert!(epsilon_squared(&same) < 0.01);
        let different = vec![vec![10.0, 11.0, 9.0, 10.5], vec![-10.0, -9.0, -11.0, -10.5]];
        assert!(epsilon_squared(&different) > 0.9);
    }
}
