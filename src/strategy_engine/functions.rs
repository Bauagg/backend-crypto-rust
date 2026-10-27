//! Fungsi yang boleh dipanggil script strategi. Semua hitungan memakai rumus di `src/indicator/`
//! (sumber yang sama dengan screener & yang dicek user di TradingView), jadi script tidak perlu
//! dan tidak bisa menulis ulang rumus indikator sendiri.
//!
//! Deret harga di script = `Array` angka, lama -> baru (index terakhir = candle terakhir). Nilai
//! yang belum ada (masa pemanasan indikator) = `NaN`.

use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, FLOAT, INT};

use crate::indicator::{
    adx, atr, bollinger_bands, ema, ichimoku, macd, obv, rsi, sma, stoch_rsi, stochastic,
    supertrend, vwap, Candle,
};

type Res<T> = Result<T, Box<EvalAltResult>>;

fn err<T>(message: impl Into<String>) -> Res<T> {
    Err(message.into().into())
}

/// Angka dari argumen script — boleh bulat (`14`) maupun desimal (`14.0`).
fn number(value: &Dynamic, name: &str) -> Res<f64> {
    if let Ok(v) = value.as_float() {
        return Ok(v);
    }
    if let Ok(v) = value.as_int() {
        return Ok(v as f64);
    }
    err(format!("{name} harus angka"))
}

/// Periode (bilangan bulat ≥ 1, maks 500).
fn period(value: &Dynamic, name: &str) -> Res<usize> {
    let v = number(value, name)?;
    if v.fract() != 0.0 || !(1.0..=500.0).contains(&v) {
        return err(format!("{name} harus bilangan bulat 1–500"));
    }
    Ok(v as usize)
}

fn to_vec(array: &Array, name: &str) -> Res<Vec<f64>> {
    array
        .iter()
        .map(|v| number(v, name))
        .collect()
}

fn to_array(values: &[Option<f64>]) -> Array {
    values.iter().map(|v| Dynamic::from_float(v.unwrap_or(f64::NAN))).collect()
}

fn plain_array(values: &[f64]) -> Array {
    values.iter().map(|v| Dynamic::from_float(*v)).collect()
}

fn map(entries: Vec<(&str, Array)>) -> Map {
    entries.into_iter().map(|(k, v)| (k.into(), Dynamic::from_array(v))).collect()
}

/// Candle dari deret high/low/close (+ volume) — panjangnya harus sama.
fn candles(high: &Array, low: &Array, close: &Array, volume: Option<&Array>) -> Res<Vec<Candle>> {
    let (high, low, close) = (to_vec(high, "high")?, to_vec(low, "low")?, to_vec(close, "close")?);
    let volume = match volume {
        Some(v) => to_vec(v, "volume")?,
        None => vec![0.0; close.len()],
    };
    if high.len() != close.len() || low.len() != close.len() || volume.len() != close.len() {
        return err("panjang deret high/low/close/volume harus sama");
    }
    Ok((0..close.len())
        .map(|i| Candle { open: close[i], high: high[i], low: low[i], close: close[i], volume: volume[i] })
        .collect())
}

fn last_value(series: &Array) -> Res<f64> {
    match series.last() {
        Some(v) => number(v, "deret"),
        None => Ok(f64::NAN),
    }
}

/// Nilai `bars` candle sebelum candle terakhir (`prev(x, 0)` = terakhir).
fn prev_value(series: &Array, bars: usize) -> Res<f64> {
    match series.len().checked_sub(bars + 1).and_then(|i| series.get(i)) {
        Some(v) => number(v, "deret"),
        None => Ok(f64::NAN),
    }
}

/// `a` memotong ke atas `b` di candle terakhir.
fn crossed(a: &Array, b: &[f64], up: bool) -> Res<bool> {
    let a = to_vec(a, "deret")?;
    let n = a.len();
    if n < 2 || b.len() < 2 {
        return Ok(false);
    }
    let (now, before) = (a[n - 1], a[n - 2]);
    let (b_now, b_before) = (b[b.len() - 1], b[b.len() - 2]);
    Ok(if up { before <= b_before && now > b_now } else { before >= b_before && now < b_now })
}

fn signal_map(score: f64, reason: String, size: Option<f64>) -> Map {
    let mut out = Map::new();
    out.insert("score".into(), Dynamic::from_float(score));
    out.insert("reason".into(), reason.into());
    if let Some(size) = size {
        out.insert("size".into(), Dynamic::from_float(size));
    }
    out
}

pub(super) fn register(engine: &mut Engine) {
    // ---- rata-rata & osilator (deret tunggal) ----
    engine.register_fn("sma", |src: Array, p: Dynamic| -> Res<Array> {
        Ok(to_array(&sma::sma(&to_vec(&src, "src")?, period(&p, "period")?)))
    });
    engine.register_fn("ema", |src: Array, p: Dynamic| -> Res<Array> {
        Ok(to_array(&ema::ema(&to_vec(&src, "src")?, period(&p, "period")?)))
    });
    engine.register_fn("rsi", |src: Array, p: Dynamic| -> Res<Array> {
        Ok(to_array(&rsi::rsi(&to_vec(&src, "src")?, period(&p, "period")?)))
    });
    engine.register_fn(
        "macd",
        |src: Array, fast: Dynamic, slow: Dynamic, signal: Dynamic| -> Res<Map> {
            let out = macd::macd(
                &to_vec(&src, "src")?,
                period(&fast, "fast")?,
                period(&slow, "slow")?,
                period(&signal, "signal")?,
            );
            Ok(map(vec![
                ("macd", to_array(&out.macd)),
                ("signal", to_array(&out.signal)),
                ("histogram", to_array(&out.histogram)),
            ]))
        },
    );
    engine.register_fn("bollinger", |src: Array, p: Dynamic, mult: Dynamic| -> Res<Map> {
        let out = bollinger_bands::bollinger_bands(&to_vec(&src, "src")?, period(&p, "period")?, number(&mult, "mult")?);
        Ok(map(vec![
            ("upper", to_array(&out.upper)),
            ("middle", to_array(&out.middle)),
            ("lower", to_array(&out.lower)),
            ("percent_b", to_array(&out.percent_b)),
            ("bandwidth", to_array(&out.bandwidth)),
        ]))
    });
    engine.register_fn(
        "stoch_rsi",
        |src: Array, rsi_p: Dynamic, stoch_p: Dynamic, smooth: Dynamic, d: Dynamic| -> Res<Map> {
            let out = stoch_rsi::stoch_rsi(
                &to_vec(&src, "src")?,
                period(&rsi_p, "rsi_period")?,
                period(&stoch_p, "stoch_period")?,
                period(&smooth, "k_smoothing")?,
                period(&d, "d_period")?,
            );
            Ok(map(vec![("k", to_array(&out.k)), ("d", to_array(&out.d))]))
        },
    );

    // ---- indikator dari candle (high/low/close/volume) ----
    engine.register_fn(
        "stochastic",
        |high: Array, low: Array, close: Array, k: Dynamic, smooth: Dynamic, d: Dynamic| -> Res<Map> {
            let out = stochastic::stochastic(
                &candles(&high, &low, &close, None)?,
                period(&k, "k_period")?,
                period(&smooth, "k_smoothing")?,
                period(&d, "d_period")?,
            );
            Ok(map(vec![("k", to_array(&out.k)), ("d", to_array(&out.d))]))
        },
    );
    engine.register_fn("atr", |high: Array, low: Array, close: Array, p: Dynamic| -> Res<Array> {
        Ok(to_array(&atr::atr(&candles(&high, &low, &close, None)?, period(&p, "period")?)))
    });
    engine.register_fn("adx", |high: Array, low: Array, close: Array, p: Dynamic| -> Res<Map> {
        let out = adx::adx(&candles(&high, &low, &close, None)?, period(&p, "period")?);
        Ok(map(vec![
            ("adx", to_array(&out.adx)),
            ("plus_di", to_array(&out.plus_di)),
            ("minus_di", to_array(&out.minus_di)),
        ]))
    });
    engine.register_fn(
        "vwap",
        |high: Array, low: Array, close: Array, volume: Array, p: Dynamic| -> Res<Array> {
            Ok(to_array(&vwap::vwap(&candles(&high, &low, &close, Some(&volume))?, period(&p, "period")?)))
        },
    );
    engine.register_fn("obv", |close: Array, volume: Array| -> Res<Array> {
        let data = candles(&close, &close, &close, Some(&volume))?;
        Ok(plain_array(&obv::obv(&data)))
    });
    engine.register_fn(
        "ichimoku",
        |high: Array, low: Array, close: Array, t: Dynamic, k: Dynamic, b: Dynamic, disp: Dynamic| -> Res<Map> {
            let out = ichimoku::ichimoku(
                &candles(&high, &low, &close, None)?,
                period(&t, "tenkan")?,
                period(&k, "kijun")?,
                period(&b, "senkou_b")?,
                period(&disp, "displacement")?,
            );
            Ok(map(vec![
                ("tenkan", to_array(&out.tenkan)),
                ("kijun", to_array(&out.kijun)),
                ("senkou_a", to_array(&out.senkou_a)),
                ("senkou_b", to_array(&out.senkou_b)),
            ]))
        },
    );
    engine.register_fn(
        "supertrend",
        |high: Array, low: Array, close: Array, p: Dynamic, mult: Dynamic| -> Res<Map> {
            let out = supertrend::supertrend(
                &candles(&high, &low, &close, None)?,
                period(&p, "period")?,
                number(&mult, "mult")?,
            );
            let uptrend: Vec<Option<f64>> =
                out.uptrend.iter().map(|u| u.map(|u| if u { 1.0 } else { 0.0 })).collect();
            Ok(map(vec![("value", to_array(&out.value)), ("uptrend", to_array(&uptrend))]))
        },
    );

    // ---- bantuan membaca deret ----
    engine.register_fn("last", |src: Array| last_value(&src));
    engine.register_fn("prev", |src: Array, bars: Dynamic| -> Res<f64> {
        let bars = number(&bars, "bars")?;
        if bars < 0.0 || bars.fract() != 0.0 {
            return err("bars harus bilangan bulat ≥ 0");
        }
        prev_value(&src, bars as usize)
    });
    engine.register_fn("highest", |src: Array, n: Dynamic| -> Res<f64> {
        let n = period(&n, "n")?;
        let values = to_vec(&src, "src")?;
        Ok(values.iter().rev().take(n).copied().filter(|v| !v.is_nan()).fold(f64::NAN, f64::max))
    });
    engine.register_fn("lowest", |src: Array, n: Dynamic| -> Res<f64> {
        let n = period(&n, "n")?;
        let values = to_vec(&src, "src")?;
        Ok(values.iter().rev().take(n).copied().filter(|v| !v.is_nan()).fold(f64::NAN, f64::min))
    });
    engine.register_fn("change_percent", |src: Array, n: Dynamic| -> Res<f64> {
        let n = period(&n, "n")?;
        let (now, before) = (last_value(&src)?, prev_value(&src, n)?);
        Ok(if before == 0.0 { f64::NAN } else { (now - before) / before * 100.0 })
    });
    engine.register_fn("crossover", |a: Array, b: Array| crossed(&a, &to_vec(&b, "deret")?, true));
    engine.register_fn("crossover", |a: Array, b: FLOAT| crossed(&a, &[b, b], true));
    engine.register_fn("crossover", |a: Array, b: INT| crossed(&a, &[b as f64, b as f64], true));
    engine.register_fn("crossunder", |a: Array, b: Array| crossed(&a, &to_vec(&b, "deret")?, false));
    engine.register_fn("crossunder", |a: Array, b: FLOAT| crossed(&a, &[b, b], false));
    engine.register_fn("crossunder", |a: Array, b: INT| crossed(&a, &[b as f64, b as f64], false));
    engine.register_fn("is_na", |v: FLOAT| v.is_nan());

    // ---- hasil script ----
    engine.register_fn("signal", |score: Dynamic, reason: &str| -> Res<Map> {
        Ok(signal_map(number(&score, "score")?, reason.to_string(), None))
    });
    engine.register_fn("signal", |score: Dynamic| -> Res<Map> {
        Ok(signal_map(number(&score, "score")?, String::new(), None))
    });
    // Ukuran posisi khusus coin ini untuk robot: persen modal robot (1–100).
    engine.register_fn("signal", |score: Dynamic, reason: &str, size: Dynamic| -> Res<Map> {
        let size = number(&size, "size")?;
        if !(1.0..=100.0).contains(&size) {
            return err("size (persen modal) harus 1–100");
        }
        Ok(signal_map(number(&score, "score")?, reason.to_string(), Some(size)))
    });
}
