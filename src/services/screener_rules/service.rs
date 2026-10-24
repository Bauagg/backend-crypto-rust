//! Rule screening (`user_screener_rules`): setelan & kondisi yang boleh dipakai tiap indikator,
//! validasi rule saat strategi disimpan, dan skor 1 rule untuk 1 coin.
//!
//! Skor tiap rule:
//! - Kondisi terpenuhi = 100. Hampir terpenuhi = sebagian (mis. RSI 34 untuk "di bawah 30" ≈ 73),
//!   supaya daftar 7–20 coin tetap terisi coin yang paling mendekati.
//! - Kondisi "cross" (memotong) = 100 kalau terjadi di candle terakhir, berkurang untuk cross yang
//!   lebih lama sampai batas `lookback` candle.

use serde_json::{json, Map, Value};
use sqlx::PgPool;

use super::model::UserScreenerRule;
use super::types::{ConditionOption, IndicatorOption, NewScreenerRule, ParamOption, RuleScore, ScreenerRuleInput};
use crate::database::{begin, commit, Tx};
use crate::indicator::{self, Candle, IndicatorKind};
use crate::services::flex_params::repository::{find_flex_param_by_id, find_flex_params_by_type};
use crate::utils::app_error::AppError;

const INDICATOR_TYPE_PARAM: &str = "INDICATORS";
const MAX_RULES_PER_SCREENER: usize = 8;

/// Jarak harga ke garis (SMA/EMA/VWAP/Supertrend) dalam persen yang masih diberi skor sebagian.
const PRICE_TOLERANCE_PERCENT: f64 = 5.0;

// =============================================================================================
// Setelan & kondisi per indikator
// =============================================================================================

struct ParamSpec {
    name: &'static str,
    label: &'static str,
    default: f64,
    min: f64,
    max: f64,
    integer: bool,
}

const fn int(name: &'static str, label: &'static str, default: usize, min: usize, max: usize) -> ParamSpec {
    ParamSpec { name, label, default: default as f64, min: min as f64, max: max as f64, integer: true }
}

const fn num(name: &'static str, label: &'static str, default: f64, min: f64, max: f64) -> ParamSpec {
    ParamSpec { name, label, default, min, max, integer: false }
}

struct ConditionSpec {
    code: &'static str,
    label: &'static str,
}

const fn cond(code: &'static str, label: &'static str) -> ConditionSpec {
    ConditionSpec { code, label }
}

struct IndicatorSpec {
    params: &'static [ParamSpec],
    conditions: &'static [ConditionSpec],
}

const LOOKBACK: ParamSpec = int("lookback", "Cross dihitung kalau terjadi dalam N candle terakhir", 3, 1, 10);

const THRESHOLD_CONDITIONS: [ConditionSpec; 4] = [
    cond("BELOW", "Di bawah batas (threshold)"),
    cond("ABOVE", "Di atas batas (threshold)"),
    cond("CROSS_UP", "Baru naik melewati batas"),
    cond("CROSS_DOWN", "Baru turun melewati batas"),
];

const STOCH_CONDITIONS: [ConditionSpec; 6] = [
    cond("BELOW", "%K di bawah batas (threshold)"),
    cond("ABOVE", "%K di atas batas (threshold)"),
    cond("CROSS_UP", "%K baru naik melewati batas"),
    cond("CROSS_DOWN", "%K baru turun melewati batas"),
    cond("K_CROSS_UP", "%K baru memotong ke atas %D"),
    cond("K_CROSS_DOWN", "%K baru memotong ke bawah %D"),
];

const PRICE_LINE_CONDITIONS: [ConditionSpec; 4] = [
    cond("PRICE_ABOVE", "Harga di atas garis"),
    cond("PRICE_BELOW", "Harga di bawah garis"),
    cond("CROSS_UP", "Harga baru menembus ke atas garis"),
    cond("CROSS_DOWN", "Harga baru menembus ke bawah garis"),
];

const MACD_CONDITIONS: [ConditionSpec; 4] = [
    cond("CROSS_UP", "MACD baru memotong ke atas garis sinyal"),
    cond("CROSS_DOWN", "MACD baru memotong ke bawah garis sinyal"),
    cond("ABOVE_SIGNAL", "MACD di atas garis sinyal"),
    cond("BELOW_SIGNAL", "MACD di bawah garis sinyal"),
];

const BOLLINGER_CONDITIONS: [ConditionSpec; 3] = [
    cond("NEAR_LOWER", "Harga dekat / di bawah pita bawah"),
    cond("NEAR_UPPER", "Harga dekat / di atas pita atas"),
    cond("SQUEEZE", "Pita menyempit (biasanya sebelum harga bergerak besar)"),
];

const ATR_CONDITIONS: [ConditionSpec; 2] = [
    cond("HIGH_VOLATILITY", "Volatilitas di atas batas"),
    cond("LOW_VOLATILITY", "Volatilitas di bawah batas"),
];

const ADX_CONDITIONS: [ConditionSpec; 4] = [
    cond("STRONG_TREND", "Tren kuat (ADX di atas batas)"),
    cond("WEAK_TREND", "Pasar datar (ADX di bawah batas)"),
    cond("STRONG_UPTREND", "Tren naik yang kuat (+DI di atas −DI)"),
    cond("STRONG_DOWNTREND", "Tren turun yang kuat (−DI di atas +DI)"),
];

const OBV_CONDITIONS: [ConditionSpec; 4] = [
    cond("RISING", "OBV naik (di atas rata-ratanya)"),
    cond("FALLING", "OBV turun (di bawah rata-ratanya)"),
    cond("CROSS_UP", "OBV baru memotong ke atas rata-ratanya"),
    cond("CROSS_DOWN", "OBV baru memotong ke bawah rata-ratanya"),
];

const ICHIMOKU_CONDITIONS: [ConditionSpec; 4] = [
    cond("PRICE_ABOVE_CLOUD", "Harga di atas awan"),
    cond("PRICE_BELOW_CLOUD", "Harga di bawah awan"),
    cond("TK_CROSS_UP", "Tenkan baru memotong ke atas Kijun"),
    cond("TK_CROSS_DOWN", "Tenkan baru memotong ke bawah Kijun"),
];

const SUPERTREND_CONDITIONS: [ConditionSpec; 4] = [
    cond("UPTREND", "Sedang tren naik"),
    cond("DOWNTREND", "Sedang tren turun"),
    cond("CROSS_UP", "Baru berbalik ke tren naik"),
    cond("CROSS_DOWN", "Baru berbalik ke tren turun"),
];

const FEAR_GREED_CONDITIONS: [ConditionSpec; 2] = [
    cond("BELOW", "Sentimen pasar di bawah batas (ketakutan)"),
    cond("ABOVE", "Sentimen pasar di atas batas (serakah)"),
];

fn spec(kind: IndicatorKind) -> IndicatorSpec {
    use crate::indicator::{
        adx, atr, bollinger_bands as bb, ema, fear_greed, ichimoku, macd, obv, rsi, sma, stoch_rsi,
        stochastic, supertrend, vwap,
    };
    const RSI: [ParamSpec; 3] = [
        int("period", "Periode", rsi::DEFAULT_PERIOD, 2, 100),
        num("threshold", "Batas", rsi::OVERSOLD, 1.0, 99.0),
        LOOKBACK,
    ];
    const MACD: [ParamSpec; 4] = [
        int("fast", "EMA cepat", macd::DEFAULT_FAST, 2, 100),
        int("slow", "EMA lambat", macd::DEFAULT_SLOW, 3, 200),
        int("signal", "Garis sinyal", macd::DEFAULT_SIGNAL, 2, 50),
        LOOKBACK,
    ];
    const SMA: [ParamSpec; 2] = [int("period", "Periode", sma::DEFAULT_PERIOD, 2, 250), LOOKBACK];
    const EMA: [ParamSpec; 2] = [int("period", "Periode", ema::DEFAULT_PERIOD, 2, 250), LOOKBACK];
    const VWAP: [ParamSpec; 2] = [int("period", "Periode (hari)", vwap::DEFAULT_PERIOD, 2, 250), LOOKBACK];
    const BB: [ParamSpec; 3] = [
        int("period", "Periode", bb::DEFAULT_PERIOD, 2, 100),
        num("multiplier", "Pengali standar deviasi", bb::DEFAULT_MULTIPLIER, 0.5, 5.0),
        int("squeeze_lookback", "Pembanding lebar pita (candle)", 120, 20, 250),
    ];
    const STOCH: [ParamSpec; 5] = [
        int("k_period", "Periode %K", stochastic::DEFAULT_K_PERIOD, 2, 100),
        int("k_smoothing", "Penghalus %K", stochastic::DEFAULT_K_SMOOTHING, 1, 20),
        int("d_period", "Periode %D", stochastic::DEFAULT_D_PERIOD, 1, 20),
        num("threshold", "Batas", stochastic::OVERSOLD, 1.0, 99.0),
        LOOKBACK,
    ];
    const STOCH_RSI: [ParamSpec; 6] = [
        int("rsi_period", "Periode RSI", stoch_rsi::DEFAULT_RSI_PERIOD, 2, 100),
        int("stoch_period", "Periode Stochastic", stoch_rsi::DEFAULT_STOCH_PERIOD, 2, 100),
        int("k_smoothing", "Penghalus %K", stoch_rsi::DEFAULT_K_SMOOTHING, 1, 20),
        int("d_period", "Periode %D", stoch_rsi::DEFAULT_D_PERIOD, 1, 20),
        num("threshold", "Batas", stoch_rsi::OVERSOLD, 1.0, 99.0),
        LOOKBACK,
    ];
    const ATR: [ParamSpec; 2] = [
        int("period", "Periode", atr::DEFAULT_PERIOD, 2, 100),
        num("threshold", "Batas volatilitas (% dari harga)", 5.0, 0.1, 50.0),
    ];
    const ADX: [ParamSpec; 2] = [
        int("period", "Periode", adx::DEFAULT_PERIOD, 2, 100),
        num("threshold", "Batas kekuatan tren", adx::STRONG_TREND, 1.0, 99.0),
    ];
    const OBV: [ParamSpec; 2] = [int("signal_period", "Periode rata-rata OBV", obv::DEFAULT_SIGNAL_PERIOD, 2, 200), LOOKBACK];
    const ICHIMOKU: [ParamSpec; 5] = [
        int("tenkan", "Tenkan-sen", ichimoku::DEFAULT_TENKAN, 2, 100),
        int("kijun", "Kijun-sen", ichimoku::DEFAULT_KIJUN, 2, 200),
        int("senkou_b", "Senkou B", ichimoku::DEFAULT_SENKOU_B, 2, 250),
        int("displacement", "Pergeseran awan", ichimoku::DEFAULT_DISPLACEMENT, 1, 100),
        LOOKBACK,
    ];
    const SUPERTREND: [ParamSpec; 3] = [
        int("atr_period", "Periode ATR", supertrend::DEFAULT_ATR_PERIOD, 2, 100),
        num("multiplier", "Pengali ATR", supertrend::DEFAULT_MULTIPLIER, 0.5, 10.0),
        LOOKBACK,
    ];
    const FEAR_GREED: [ParamSpec; 2] = [
        int("period", "Rata-rata (hari)", 1, 1, fear_greed::DEFAULT_PERIOD * 2),
        num("threshold", "Batas", 25.0, 1.0, 99.0),
    ];

    match kind {
        IndicatorKind::Rsi => IndicatorSpec { params: &RSI, conditions: &THRESHOLD_CONDITIONS },
        IndicatorKind::Macd => IndicatorSpec { params: &MACD, conditions: &MACD_CONDITIONS },
        IndicatorKind::Sma => IndicatorSpec { params: &SMA, conditions: &PRICE_LINE_CONDITIONS },
        IndicatorKind::Ema => IndicatorSpec { params: &EMA, conditions: &PRICE_LINE_CONDITIONS },
        IndicatorKind::Vwap => IndicatorSpec { params: &VWAP, conditions: &PRICE_LINE_CONDITIONS },
        IndicatorKind::BollingerBands => IndicatorSpec { params: &BB, conditions: &BOLLINGER_CONDITIONS },
        IndicatorKind::Stochastic => IndicatorSpec { params: &STOCH, conditions: &STOCH_CONDITIONS },
        IndicatorKind::StochRsi => IndicatorSpec { params: &STOCH_RSI, conditions: &STOCH_CONDITIONS },
        IndicatorKind::Atr => IndicatorSpec { params: &ATR, conditions: &ATR_CONDITIONS },
        IndicatorKind::Adx => IndicatorSpec { params: &ADX, conditions: &ADX_CONDITIONS },
        IndicatorKind::Obv => IndicatorSpec { params: &OBV, conditions: &OBV_CONDITIONS },
        IndicatorKind::Ichimoku => IndicatorSpec { params: &ICHIMOKU, conditions: &ICHIMOKU_CONDITIONS },
        IndicatorKind::Supertrend => IndicatorSpec { params: &SUPERTREND, conditions: &SUPERTREND_CONDITIONS },
        IndicatorKind::FearGreed => IndicatorSpec { params: &FEAR_GREED, conditions: &FEAR_GREED_CONDITIONS },
    }
}

/// Setelan rule yang sudah tersimpan; key yang tidak ada (rule lama) memakai default.
struct Params<'a> {
    values: &'a Value,
    spec: &'static [ParamSpec],
}

impl Params<'_> {
    fn get(&self, name: &str) -> f64 {
        self.values
            .get(name)
            .and_then(Value::as_f64)
            .or_else(|| self.spec.iter().find(|p| p.name == name).map(|p| p.default))
            .unwrap_or_default()
    }

    fn period(&self, name: &str) -> usize {
        self.get(name).max(1.0) as usize
    }
}

/// Lengkapi `params` dengan default & tolak key/angka yang tidak valid.
fn normalize_params(code: &str, spec: &IndicatorSpec, raw: &Value) -> Result<Value, AppError> {
    let empty = Map::new();
    let given = match raw {
        Value::Null => &empty,
        Value::Object(map) => map,
        _ => return Err(AppError::BadRequest(format!("params {code} harus berupa object"))),
    };
    if let Some(unknown) = given.keys().find(|k| !spec.params.iter().any(|p| p.name == k.as_str())) {
        let allowed: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
        return Err(AppError::BadRequest(format!(
            "params {code} tidak mengenal '{unknown}' (yang boleh: {})",
            allowed.join(", ")
        )));
    }

    let mut normalized = Map::new();
    for param in spec.params {
        let value = match given.get(param.name) {
            None | Some(Value::Null) => param.default,
            Some(v) => v.as_f64().ok_or_else(|| {
                AppError::BadRequest(format!("params {code}.{} harus angka", param.name))
            })?,
        };
        if value < param.min || value > param.max {
            return Err(AppError::BadRequest(format!(
                "params {code}.{} harus antara {} dan {}",
                param.name, param.min, param.max
            )));
        }
        if param.integer && value.fract() != 0.0 {
            return Err(AppError::BadRequest(format!("params {code}.{} harus bilangan bulat", param.name)));
        }
        let value = if param.integer { json!(value as i64) } else { json!(value) };
        normalized.insert(param.name.to_string(), value);
    }

    let get = |name: &str| normalized.get(name).and_then(Value::as_f64).unwrap_or_default();
    if code == "MACD" && get("fast") >= get("slow") {
        return Err(AppError::BadRequest("params MACD.fast harus lebih kecil dari slow".to_string()));
    }
    if code == "ICHIMOKU" && !(get("tenkan") < get("kijun") && get("kijun") < get("senkou_b")) {
        return Err(AppError::BadRequest(
            "params ICHIMOKU harus tenkan < kijun < senkou_b".to_string(),
        ));
    }
    Ok(Value::Object(normalized))
}

/// Katalog indikator untuk asisten AI: yang ada di flex_params `INDICATORS` (aktif) DAN sudah punya
/// rumus di `src/indicator/` — indikator tanpa rumus tidak pernah ditawarkan AI.
pub async fn get_indicator_options_service(pool: &PgPool) -> Result<Vec<IndicatorOption>, AppError> {
    let mut tx = begin(pool).await?;
    let params = find_flex_params_by_type(&mut tx, INDICATOR_TYPE_PARAM, true).await?;
    commit(tx).await?;

    let mut options = Vec::new();
    for param in params {
        let Some(kind) = IndicatorKind::from_code(&param.value_param) else {
            tracing::warn!(
                "Indikator {} ada di flex_params tapi belum ada rumusnya di src/indicator/",
                param.value_param
            );
            continue;
        };
        let spec = spec(kind);
        options.push(IndicatorOption {
            flex_param_id: param.id,
            code: kind.code().to_string(),
            description: param.description,
            conditions: spec.conditions.iter().map(|c| ConditionOption { code: c.code, label: c.label }).collect(),
            params: spec
                .params
                .iter()
                .map(|p| ParamOption {
                    name: p.name,
                    label: p.label,
                    default: p.default,
                    min: p.min,
                    max: p.max,
                    integer: p.integer,
                })
                .collect(),
        });
    }
    Ok(options)
}

/// Validasi daftar rule strategi & lengkapi default `params`-nya, di dalam transaksi pemanggil
/// (indikator dibaca dari flex_params).
pub async fn build_rules_in_tx(
    tx: &mut Tx<'_>,
    inputs: &[ScreenerRuleInput],
) -> Result<Vec<NewScreenerRule>, AppError> {
    if inputs.is_empty() {
        return Err(AppError::BadRequest("Strategi minimal berisi 1 indikator".to_string()));
    }
    if inputs.len() > MAX_RULES_PER_SCREENER {
        return Err(AppError::BadRequest(format!(
            "Strategi maksimal berisi {MAX_RULES_PER_SCREENER} indikator"
        )));
    }

    let mut rules = Vec::with_capacity(inputs.len());
    for (index, rule) in inputs.iter().enumerate() {
        let param = find_flex_param_by_id(tx, rule.flex_param_id)
            .await?
            .filter(|p| p.type_param == INDICATOR_TYPE_PARAM && p.is_active)
            .ok_or_else(|| AppError::BadRequest(format!("Indikator ke-{} tidak ditemukan", index + 1)))?;
        let kind = IndicatorKind::from_code(&param.value_param).ok_or_else(|| {
            AppError::BadRequest(format!("Indikator {} belum didukung", param.value_param))
        })?;
        let code = kind.code();
        let spec = spec(kind);

        let condition = rule.condition.trim().to_uppercase();
        if !spec.conditions.iter().any(|c| c.code == condition) {
            let allowed: Vec<&str> = spec.conditions.iter().map(|c| c.code).collect();
            return Err(AppError::BadRequest(format!(
                "Kondisi {code} harus salah satu dari: {}",
                allowed.join(", ")
            )));
        }
        let weight = rule.weight.unwrap_or(1);
        if !(1..=5).contains(&weight) {
            return Err(AppError::BadRequest("weight harus 1–5".to_string()));
        }

        rules.push(NewScreenerRule {
            flex_param_id: param.id,
            indicator_code: code.to_string(),
            params: normalize_params(code, &spec, &rule.params)?,
            condition,
            weight,
            sort_order: index as i16,
        });
    }
    Ok(rules)
}

/// Indikator rule tersimpan; rule yang indikatornya sudah tidak punya rumus ditolak.
pub fn kind_of(rule: &UserScreenerRule) -> Result<IndicatorKind, AppError> {
    IndicatorKind::from_code(&rule.indicator_code).ok_or_else(|| {
        AppError::BadRequest(format!(
            "Indikator {} sudah tidak didukung — ubah strategi ini",
            rule.indicator_code
        ))
    })
}

/// Skor 1 rule (0–100) untuk 1 coin + nilai indikatornya di candle terakhir. `None` kalau histori
/// candle belum cukup. `fear_greed` = nilai harian Fear & Greed (lama -> baru), hanya dipakai rule
/// FEAR_GREED.
pub fn score_rule(
    kind: IndicatorKind,
    rule: &UserScreenerRule,
    candles: &[Candle],
    fear_greed: &[f64],
) -> Option<RuleScore> {
    let params = Params { values: &rule.params, spec: spec(kind).params };
    let (score, values) = evaluate(kind, &rule.condition, &params, candles, fear_greed)?;
    Some(RuleScore {
        rule_id: rule.id,
        indicator_code: rule.indicator_code.clone(),
        condition: rule.condition.clone(),
        weight: rule.weight,
        score: round(score.clamp(0.0, 100.0), 2),
        values,
    })
}

fn round(value: f64, decimals: i32) -> f64 {
    let factor = 10f64.powi(decimals);
    (value * factor).round() / factor
}

fn last(series: &[Option<f64>]) -> Option<f64> {
    *series.last()?
}

/// Nilai `back` candle sebelum yang terakhir.
fn previous(series: &[Option<f64>], back: usize) -> Option<f64> {
    *series.get(series.len().checked_sub(back + 1)?)?
}

/// 100 kalau `value` ≤ `threshold`, turun linear sampai 0 di `threshold + tolerance`.
fn below_score(value: f64, threshold: f64, tolerance: f64) -> f64 {
    if value <= threshold {
        100.0
    } else {
        (100.0 * (1.0 - (value - threshold) / tolerance)).max(0.0)
    }
}

fn above_score(value: f64, threshold: f64, tolerance: f64) -> f64 {
    if value >= threshold {
        100.0
    } else {
        (100.0 * (1.0 - (threshold - value) / tolerance)).max(0.0)
    }
}

/// Berapa candle lalu `series` memotong `level` (ke atas kalau `up`), dicari di `lookback` candle
/// terakhir. 0 = di candle terakhir.
fn bars_since_cross(
    series: &[Option<f64>],
    level: impl Fn(usize) -> Option<f64>,
    up: bool,
    lookback: usize,
) -> Option<usize> {
    let len = series.len();
    (0..lookback.min(len.saturating_sub(1))).find(|&k| {
        let i = len - 1 - k;
        let (Some(now), Some(before), Some(level_now), Some(level_before)) =
            (series[i], series[i - 1], level(i), level(i - 1))
        else {
            return false;
        };
        if up {
            before <= level_before && now > level_now
        } else {
            before >= level_before && now < level_now
        }
    })
}

/// Cross di candle terakhir = 100, makin lama makin kecil (cross `lookback − 1` candle lalu ≈ 50+).
fn cross_score(bars_ago: usize, lookback: usize) -> f64 {
    100.0 - bars_ago as f64 * 50.0 / lookback.max(1) as f64
}

/// Osilator 0–100 (RSI, %K Stochastic, %K Stoch RSI) terhadap batas, atau %K terhadap %D.
fn oscillator(
    name: &str,
    main: &[Option<f64>],
    signal: Option<&[Option<f64>]>,
    condition: &str,
    params: &Params<'_>,
    tolerance: f64,
) -> Option<(f64, Value)> {
    let value = last(main)?;
    let threshold = params.get("threshold");
    let lookback = params.period("lookback");
    let mut values = Map::new();
    values.insert(name.to_string(), json!(round(value, 2)));
    let signal_now = signal.and_then(last);
    if let Some(d) = signal_now {
        values.insert("d".to_string(), json!(round(d, 2)));
    }

    let score = match condition {
        "BELOW" => below_score(value, threshold, tolerance),
        "ABOVE" => above_score(value, threshold, tolerance),
        "CROSS_UP" | "CROSS_DOWN" => {
            let up = condition == "CROSS_UP";
            match bars_since_cross(main, |_| Some(threshold), up, lookback) {
                Some(k) => {
                    values.insert("bars_since_cross".to_string(), json!(k));
                    cross_score(k, lookback)
                }
                // Belum memotong: makin dekat ke batas dari sisi yang benar, makin besar (maks 50).
                None if up && value < threshold => 0.5 * above_score(value, threshold, tolerance),
                None if !up && value > threshold => 0.5 * below_score(value, threshold, tolerance),
                None => 0.0,
            }
        }
        "K_CROSS_UP" | "K_CROSS_DOWN" => {
            let d = signal?;
            signal_now?;
            match bars_since_cross(main, |i| d[i], condition == "K_CROSS_UP", lookback) {
                Some(k) => {
                    values.insert("bars_since_cross".to_string(), json!(k));
                    cross_score(k, lookback)
                }
                None => 0.0,
            }
        }
        _ => return None,
    };
    Some((score, Value::Object(values)))
}

/// Harga close terhadap garis (SMA/EMA/VWAP).
fn price_vs_line(name: &str, closes: &[f64], line: &[Option<f64>], condition: &str, lookback: usize) -> Option<(f64, Value)> {
    let line_now = last(line)?;
    let close = *closes.last()?;
    if line_now == 0.0 {
        return None;
    }
    let distance = (close - line_now) / line_now * 100.0;
    let above = above_score(distance, 0.0, PRICE_TOLERANCE_PERCENT);
    let below = below_score(distance, 0.0, PRICE_TOLERANCE_PERCENT);
    let mut values = Map::new();
    values.insert(name.to_string(), json!(round(line_now, 8)));
    values.insert("distance_percent".to_string(), json!(round(distance, 2)));

    let price: Vec<Option<f64>> = closes.iter().copied().map(Some).collect();
    let score = match condition {
        "PRICE_ABOVE" => above,
        "PRICE_BELOW" => below,
        "CROSS_UP" | "CROSS_DOWN" => {
            let up = condition == "CROSS_UP";
            match bars_since_cross(&price, |i| line[i], up, lookback) {
                Some(k) => {
                    values.insert("bars_since_cross".to_string(), json!(k));
                    cross_score(k, lookback)
                }
                None if up && distance < 0.0 => 0.5 * above,
                None if !up && distance > 0.0 => 0.5 * below,
                None => 0.0,
            }
        }
        _ => return None,
    };
    Some((score, Value::Object(values)))
}

/// Skor 1 rule untuk 1 coin + nilai indikatornya. `None` = data belum cukup.
fn evaluate(
    kind: IndicatorKind,
    condition: &str,
    params: &Params<'_>,
    candles: &[Candle],
    fear_greed: &[f64],
) -> Option<(f64, Value)> {
    use crate::indicator::{
        adx, atr, bollinger_bands, ema, ichimoku, macd, obv, rsi, sma, stoch_rsi, stochastic,
        supertrend, vwap,
    };
    let closes = indicator::closes(candles);
    let lookback = params.period("lookback");

    match kind {
        IndicatorKind::Rsi => {
            let values = rsi::rsi(&closes, params.period("period"));
            oscillator("rsi", &values, None, condition, params, 15.0)
        }
        IndicatorKind::Stochastic => {
            let out = stochastic::stochastic(
                candles,
                params.period("k_period"),
                params.period("k_smoothing"),
                params.period("d_period"),
            );
            oscillator("k", &out.k, Some(&out.d), condition, params, 25.0)
        }
        IndicatorKind::StochRsi => {
            let out = stoch_rsi::stoch_rsi(
                &closes,
                params.period("rsi_period"),
                params.period("stoch_period"),
                params.period("k_smoothing"),
                params.period("d_period"),
            );
            oscillator("k", &out.k, Some(&out.d), condition, params, 25.0)
        }
        IndicatorKind::Macd => {
            let out = macd::macd(&closes, params.period("fast"), params.period("slow"), params.period("signal"));
            let (line, signal, histogram) = (last(&out.macd)?, last(&out.signal)?, last(&out.histogram)?);
            let rising = previous(&out.histogram, 1).is_some_and(|before| histogram > before);
            let mut values = Map::new();
            values.insert("macd".to_string(), json!(round(line, 8)));
            values.insert("signal".to_string(), json!(round(signal, 8)));
            values.insert("histogram".to_string(), json!(round(histogram, 8)));
            let score = match condition {
                "CROSS_UP" | "CROSS_DOWN" => {
                    let up = condition == "CROSS_UP";
                    match bars_since_cross(&out.macd, |i| out.signal[i], up, lookback) {
                        Some(k) => {
                            values.insert("bars_since_cross".to_string(), json!(k));
                            cross_score(k, lookback)
                        }
                        // Belum memotong tapi histogram sedang mendekat ke garis sinyal.
                        None if up && histogram < 0.0 && rising => 40.0,
                        None if !up && histogram > 0.0 && !rising => 40.0,
                        None => 0.0,
                    }
                }
                "ABOVE_SIGNAL" if histogram > 0.0 => 100.0,
                "ABOVE_SIGNAL" if rising => 40.0,
                "BELOW_SIGNAL" if histogram < 0.0 => 100.0,
                "BELOW_SIGNAL" if !rising => 40.0,
                "ABOVE_SIGNAL" | "BELOW_SIGNAL" => 0.0,
                _ => return None,
            };
            Some((score, Value::Object(values)))
        }
        IndicatorKind::Sma => {
            let line = sma::sma(&closes, params.period("period"));
            price_vs_line("sma", &closes, &line, condition, lookback)
        }
        IndicatorKind::Ema => {
            let line = ema::ema(&closes, params.period("period"));
            price_vs_line("ema", &closes, &line, condition, lookback)
        }
        IndicatorKind::Vwap => {
            let line = vwap::vwap(candles, params.period("period"));
            price_vs_line("vwap", &closes, &line, condition, lookback)
        }
        IndicatorKind::BollingerBands => {
            let out = bollinger_bands::bollinger_bands(&closes, params.period("period"), params.get("multiplier"));
            let percent_b = last(&out.percent_b)?;
            let bandwidth = last(&out.bandwidth)?;
            let mut values = Map::new();
            values.insert("upper".to_string(), json!(round(last(&out.upper)?, 8)));
            values.insert("middle".to_string(), json!(round(last(&out.middle)?, 8)));
            values.insert("lower".to_string(), json!(round(last(&out.lower)?, 8)));
            values.insert("percent_b".to_string(), json!(round(percent_b, 4)));
            values.insert("bandwidth".to_string(), json!(round(bandwidth, 4)));
            let score = match condition {
                // %B 0 = tepat di pita bawah (100), %B 0,5 = di tengah (0).
                "NEAR_LOWER" => below_score(percent_b, 0.0, 0.5),
                "NEAR_UPPER" => above_score(percent_b, 1.0, 0.5),
                "SQUEEZE" => {
                    // Lebar pita sekarang dibanding `squeeze_lookback` candle terakhir: tersempit = 100.
                    let window = params.period("squeeze_lookback");
                    let recent: Vec<f64> = out.bandwidth.iter().rev().take(window).flatten().copied().collect();
                    let narrower = recent.iter().filter(|&&b| b < bandwidth).count();
                    values.insert("bandwidth_rank_percent".to_string(), json!(round(narrower as f64 / recent.len() as f64 * 100.0, 2)));
                    100.0 * (1.0 - narrower as f64 / recent.len() as f64)
                }
                _ => return None,
            };
            Some((score, Value::Object(values)))
        }
        IndicatorKind::Atr => {
            let value = last(&atr::atr_percent(candles, params.period("period")))?;
            let threshold = params.get("threshold");
            let score = match condition {
                "HIGH_VOLATILITY" => above_score(value, threshold, threshold),
                "LOW_VOLATILITY" => below_score(value, threshold, threshold),
                _ => return None,
            };
            Some((score, json!({ "atr_percent": round(value, 2) })))
        }
        IndicatorKind::Adx => {
            let out = adx::adx(candles, params.period("period"));
            let (value, plus, minus) = (last(&out.adx)?, last(&out.plus_di)?, last(&out.minus_di)?);
            let threshold = params.get("threshold");
            let strong = above_score(value, threshold, 10.0);
            let score = match condition {
                "STRONG_TREND" => strong,
                "WEAK_TREND" => below_score(value, threshold, 10.0),
                "STRONG_UPTREND" if plus > minus => strong,
                "STRONG_DOWNTREND" if minus > plus => strong,
                "STRONG_UPTREND" | "STRONG_DOWNTREND" => 0.0,
                _ => return None,
            };
            Some((
                score,
                json!({ "adx": round(value, 2), "plus_di": round(plus, 2), "minus_di": round(minus, 2) }),
            ))
        }
        IndicatorKind::Obv => {
            let line: Vec<Option<f64>> = obv::obv(candles).into_iter().map(Some).collect();
            let signal = obv::obv_signal(candles, params.period("signal_period"));
            let (value, average) = (last(&line)?, last(&signal)?);
            let rising = previous(&line, 1).is_some_and(|before| value > before);
            let mut values = Map::new();
            values.insert("obv".to_string(), json!(round(value, 2)));
            values.insert("obv_average".to_string(), json!(round(average, 2)));
            let score = match condition {
                "RISING" if value > average => 100.0,
                "RISING" if rising => 40.0,
                "FALLING" if value < average => 100.0,
                "FALLING" if !rising => 40.0,
                "RISING" | "FALLING" => 0.0,
                "CROSS_UP" | "CROSS_DOWN" => {
                    match bars_since_cross(&line, |i| signal[i], condition == "CROSS_UP", lookback) {
                        Some(k) => {
                            values.insert("bars_since_cross".to_string(), json!(k));
                            cross_score(k, lookback)
                        }
                        None => 0.0,
                    }
                }
                _ => return None,
            };
            Some((score, Value::Object(values)))
        }
        IndicatorKind::Ichimoku => {
            let out = ichimoku::ichimoku(
                candles,
                params.period("tenkan"),
                params.period("kijun"),
                params.period("senkou_b"),
                params.period("displacement"),
            );
            let index = candles.len().checked_sub(1)?;
            let (top, bottom) = out.cloud_at(index)?;
            let (tenkan, kijun) = (last(&out.tenkan)?, last(&out.kijun)?);
            let close = *closes.last()?;
            let mut values = Map::new();
            values.insert("tenkan".to_string(), json!(round(tenkan, 8)));
            values.insert("kijun".to_string(), json!(round(kijun, 8)));
            values.insert("cloud_top".to_string(), json!(round(top, 8)));
            values.insert("cloud_bottom".to_string(), json!(round(bottom, 8)));
            let in_cloud = close <= top && close >= bottom;
            let score = match condition {
                "PRICE_ABOVE_CLOUD" if close > top => 100.0,
                "PRICE_BELOW_CLOUD" if close < bottom => 100.0,
                "PRICE_ABOVE_CLOUD" | "PRICE_BELOW_CLOUD" if in_cloud => 50.0,
                "PRICE_ABOVE_CLOUD" | "PRICE_BELOW_CLOUD" => 0.0,
                "TK_CROSS_UP" | "TK_CROSS_DOWN" => {
                    let up = condition == "TK_CROSS_UP";
                    match bars_since_cross(&out.tenkan, |i| out.kijun[i], up, lookback) {
                        Some(k) => {
                            values.insert("bars_since_cross".to_string(), json!(k));
                            cross_score(k, lookback)
                        }
                        // Sudah di sisi yang benar tapi cross-nya lama.
                        None if (up && tenkan > kijun) || (!up && tenkan < kijun) => 50.0,
                        None => 0.0,
                    }
                }
                _ => return None,
            };
            Some((score, Value::Object(values)))
        }
        IndicatorKind::Supertrend => {
            let out = supertrend::supertrend(candles, params.period("atr_period"), params.get("multiplier"));
            let (line, up) = (last(&out.value)?, (*out.uptrend.last()?)?);
            let close = *closes.last()?;
            let mut values = Map::new();
            values.insert("supertrend".to_string(), json!(round(line, 8)));
            values.insert("uptrend".to_string(), json!(up));
            let trend: Vec<Option<f64>> = out.uptrend.iter().map(|u| u.map(|u| if u { 1.0 } else { 0.0 })).collect();
            // Jarak ke garis saat tren belum sesuai: makin dekat, makin mungkin berbalik (maks 50).
            let distance = if line == 0.0 { f64::MAX } else { (close - line).abs() / line * 100.0 };
            let near = 0.5 * below_score(distance, 0.0, PRICE_TOLERANCE_PERCENT);
            let score = match condition {
                "UPTREND" if up => 100.0,
                "DOWNTREND" if !up => 100.0,
                "UPTREND" | "DOWNTREND" => near,
                "CROSS_UP" | "CROSS_DOWN" => {
                    match bars_since_cross(&trend, |_| Some(0.5), condition == "CROSS_UP", lookback) {
                        Some(k) => {
                            values.insert("bars_since_cross".to_string(), json!(k));
                            cross_score(k, lookback)
                        }
                        None => 0.0,
                    }
                }
                _ => return None,
            };
            Some((score, Value::Object(values)))
        }
        IndicatorKind::FearGreed => {
            let period = params.period("period").min(fear_greed.len());
            let value = last(&indicator::fear_greed::average(fear_greed, period))?;
            let threshold = params.get("threshold");
            let score = match condition {
                "BELOW" => below_score(value, threshold, 15.0),
                "ABOVE" => above_score(value, threshold, 15.0),
                _ => return None,
            };
            let zone = indicator::fear_greed::Zone::of(value).code();
            Some((score, json!({ "fear_greed": round(value, 2), "zone": zone })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rising_candles(count: usize) -> Vec<Candle> {
        (0..count)
            .map(|i| {
                let close = 100.0 + i as f64;
                Candle { open: close - 0.5, high: close + 1.0, low: close - 1.0, close, volume: 1000.0 }
            })
            .collect()
    }

    #[test]
    fn default_params_valid_untuk_semua_indikator() {
        for kind in IndicatorKind::ALL {
            let spec = spec(kind);
            assert!(!spec.conditions.is_empty(), "{kind:?} tanpa kondisi");
            normalize_params(kind.code(), &spec, &Value::Null).unwrap_or_else(|e| panic!("{kind:?}: {e:?}"));
        }
    }

    #[test]
    fn semua_kondisi_bisa_dinilai() {
        let candles = rising_candles(300);
        let fear_greed = vec![20.0; 30];
        for kind in IndicatorKind::ALL {
            let spec = spec(kind);
            let defaults = normalize_params(kind.code(), &spec, &Value::Null).unwrap();
            let params = Params { values: &defaults, spec: spec.params };
            for condition in spec.conditions {
                let (score, _) = evaluate(kind, condition.code, &params, &candles, &fear_greed)
                    .unwrap_or_else(|| panic!("{kind:?} {} tidak bisa dinilai", condition.code));
                assert!((0.0..=100.0).contains(&score), "{kind:?} {} skor {score}", condition.code);
            }
        }
    }

    #[test]
    fn tolak_params_tidak_valid() {
        let rsi = spec(IndicatorKind::Rsi);
        assert!(normalize_params("RSI", &rsi, &json!({"periode": 14})).is_err());
        assert!(normalize_params("RSI", &rsi, &json!({"period": 1})).is_err());
        assert!(normalize_params("RSI", &rsi, &json!({"period": 13.5})).is_err());
        let ok = normalize_params("RSI", &rsi, &json!({"period": 13})).unwrap();
        assert_eq!(ok, json!({"period": 13, "threshold": 30.0, "lookback": 3}));

        let macd = spec(IndicatorKind::Macd);
        assert!(normalize_params("MACD", &macd, &json!({"fast": 30, "slow": 26})).is_err());
    }

    #[test]
    fn skor_mendekati_batas() {
        assert_eq!(below_score(25.0, 30.0, 15.0), 100.0);
        assert!((below_score(34.0, 30.0, 15.0) - 73.333).abs() < 0.01);
        assert_eq!(below_score(50.0, 30.0, 15.0), 0.0);
        assert_eq!(cross_score(0, 3), 100.0);
    }

    #[test]
    fn harga_naik_terus_cocok_tren_naik() {
        let candles = rising_candles(300);
        let sma = spec(IndicatorKind::Sma);
        let defaults = normalize_params("SMA", &sma, &Value::Null).unwrap();
        let params = Params { values: &defaults, spec: sma.params };
        let (above, _) = evaluate(IndicatorKind::Sma, "PRICE_ABOVE", &params, &candles, &[]).unwrap();
        let (below, _) = evaluate(IndicatorKind::Sma, "PRICE_BELOW", &params, &candles, &[]).unwrap();
        assert_eq!(above, 100.0);
        assert!(below < 100.0);

        let rsi = spec(IndicatorKind::Rsi);
        let defaults = normalize_params("RSI", &rsi, &json!({"threshold": 70})).unwrap();
        let params = Params { values: &defaults, spec: rsi.params };
        let (score, values) = evaluate(IndicatorKind::Rsi, "ABOVE", &params, &candles, &[]).unwrap();
        assert_eq!(score, 100.0);
        assert_eq!(values["rsi"], json!(100.0));
    }
}
