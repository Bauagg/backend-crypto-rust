//! Runtime script strategi buatan user (bahasa Rhai), dijalankan di sandbox:
//! - script hanya bisa memanggil fungsi yang didaftarkan di `functions.rs` (rumus `src/indicator/`)
//!   — tidak ada akses file, jaringan, database, `import`, atau `eval`;
//! - dibatasi jumlah operasi (loop tanpa akhir otomatis dihentikan), kedalaman panggilan, dan
//!   ukuran string/array/map — script rusak hanya gagal sendiri, server tetap jalan;
//! - `print`/`debug` dibuang.
//!
//! Seperti `indicator/`, folder ini hanya hitungan: tidak membaca DB/API. Pemanggil (service)
//! yang menyiapkan candle & menyimpan hasilnya. Script dijalankan sinkron (CPU) — panggil dari
//! `tokio::task::spawn_blocking`.
//!
//! Kontrak script (lihat `API_REFERENCE`): dijalankan sekali per coin dengan deret `open`, `high`,
//! `low`, `close`, `volume` (candle harian, lama -> baru), lalu mengembalikan skor 0–100 (atau
//! `signal(skor, "alasan")`).

mod functions;
pub mod inputs;

use rhai::{Array, Dynamic, Engine, Map, Scope, AST};
use serde_json::Value;

use crate::indicator::Candle;

/// Bahasa script yang didukung (`strategy_versions.language`).
pub const LANGUAGE: &str = "RHAI";
/// Bentuk hasil script (`strategy_versions.output_type`).
pub const OUTPUT_TYPE: &str = "SCORE";
/// Data candle yang dipakai script (`strategy_versions.timeframe`).
pub const TIMEFRAME: &str = "1D";
pub const MAX_SOURCE_CHARS: usize = 20_000;

/// Batas operasi 1x eksekusi script untuk 1 coin (±puluhan ms) — loop tanpa akhir dihentikan.
const MAX_OPERATIONS: u64 = 1_000_000;

/// Dokumentasi API script — dipakai prompt asisten AI & bisa ditampilkan FE. Satu sumber, supaya
/// yang dijelaskan selalu sama dengan yang didaftarkan di `functions.rs`.
pub const API_REFERENCE: &str = r#"BAHASA: Rhai (mirip JavaScript/Rust). Script dijalankan SEKALI PER COIN dengan data candle HARIAN (D1).

VARIABEL TERSEDIA:
- open, high, low, close, volume : array angka, urut lama -> baru (index terakhir = candle terakhir)
- symbol      : simbol coin, mis. "BTCUSDT"
- inputs      : setelan strategi, mis. inputs.rsi_period (dideklarasikan di `inputs`)
- fear_greed  : Fear & Greed Index terakhir (0–100, sentimen seluruh pasar), NaN kalau tidak ada

FUNGSI INDIKATOR (hasil array sepanjang data; nilai yang belum ada = NaN):
- sma(src, period), ema(src, period), rsi(src, period)
- macd(src, fast, slow, signal)                     -> #{macd, signal, histogram}
- bollinger(src, period, mult)                      -> #{upper, middle, lower, percent_b, bandwidth}
- stoch_rsi(src, rsi_period, stoch_period, k_smoothing, d_period) -> #{k, d}
- stochastic(high, low, close, k_period, k_smoothing, d_period)   -> #{k, d}
- atr(high, low, close, period)
- adx(high, low, close, period)                     -> #{adx, plus_di, minus_di}
- vwap(high, low, close, volume, period)
- obv(close, volume)
- ichimoku(high, low, close, tenkan, kijun, senkou_b, displacement) -> #{tenkan, kijun, senkou_a, senkou_b}
- supertrend(high, low, close, period, mult)        -> #{value, uptrend}  (uptrend: 1 = naik, 0 = turun)

FUNGSI BANTUAN:
- last(arr)               nilai terakhir
- prev(arr, n)            nilai n candle sebelum terakhir (prev(arr, 0) = terakhir)
- highest(arr, n), lowest(arr, n)   tertinggi / terendah n candle terakhir
- change_percent(arr, n)  perubahan % dari n candle lalu
- crossover(a, b), crossunder(a, b) a memotong ke atas / bawah b DI CANDLE TERAKHIR (b = array atau angka)
- is_na(x)                true kalau x NaN (data belum cukup)

HASIL (ekspresi terakhir script):
- angka skor 0–100 (100 = paling cocok), ATAU
- signal(skor, "alasan singkat")  -> skor + alasan yang ditampilkan ke user, ATAU
- signal(skor, "alasan", persen)  -> + ukuran posisi khusus coin ini saat dipakai sebagai ROBOT:
                                     persen modal robot (1–100). Tanpa persen = position_percent strategi.

ROBOT: coin DIBELI kalau skor >= entry_score dan DIJUAL kalau skor <= exit_score (diatur di strategi),
maksimal max_positions coin bersamaan. Sinyal dari candle close hari ini, order dijalankan besoknya.

CONTOH:
let r = last(rsi(close, inputs.rsi_period));
let tren_naik = last(close) > last(ema(close, 200));
if tren_naik && r < 35 {
    signal(90, "RSI murah di tren naik")
} else if tren_naik && r < 45 {
    signal(60, "agak murah, tren naik")
} else {
    signal(0, "belum cocok")
}

BATASAN: tidak ada akses file/jaringan/database, tidak ada import/eval, maks 1.000.000 operasi per coin."#;

mod backtest;
mod insights;
mod validation;
pub use backtest::{run_backtest, BacktestConfig, BacktestOutput, BacktestTrade, SymbolHistory};

/// Hasil script untuk 1 coin.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptOutput {
    /// 0–100.
    pub score: f64,
    pub reason: String,
    /// Ukuran posisi robot untuk coin ini (persen modal robot), kalau script menentukannya.
    pub size_percent: Option<f64>,
}

fn engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .set_max_operations(MAX_OPERATIONS)
        .set_max_call_levels(32)
        .set_max_expr_depths(64, 32)
        .set_max_string_size(2_000)
        .set_max_array_size(10_000)
        .set_max_map_size(200);
    engine.disable_symbol("eval");
    engine.on_print(|_| {});
    engine.on_debug(|_, _, _| {});
    functions::register(&mut engine);
    engine
}

/// Script yang sudah lolos compile, siap dijalankan berkali-kali (per coin).
#[derive(Debug)]
pub struct CompiledScript {
    engine: Engine,
    ast: AST,
}

/// Compile script. Error = pesan untuk user (termasuk baris & posisi).
pub fn compile(source: &str) -> Result<CompiledScript, String> {
    if source.trim().is_empty() {
        return Err("Script kosong".to_string());
    }
    if source.chars().count() > MAX_SOURCE_CHARS {
        return Err(format!("Script maksimal {MAX_SOURCE_CHARS} karakter"));
    }
    let engine = engine();
    let ast = engine.compile(source).map_err(|e| format!("Error sintaks: {e}"))?;
    Ok(CompiledScript { engine, ast })
}

/// Data 1 coin untuk dijalankan script.
pub struct CoinData<'a> {
    pub symbol: &'a str,
    pub candles: &'a [Candle],
}

impl CompiledScript {
    /// Jalankan script untuk 1 coin. `input_values` = hasil `inputs::resolve_values`.
    pub fn run(&self, coin: &CoinData<'_>, input_values: &Value, fear_greed: Option<f64>) -> Result<ScriptOutput, String> {
        let series = |pick: fn(&Candle) -> f64| -> Array {
            coin.candles.iter().map(|c| Dynamic::from_float(pick(c))).collect()
        };
        let mut scope = Scope::new();
        scope
            .push_constant("open", series(|c| c.open))
            .push_constant("high", series(|c| c.high))
            .push_constant("low", series(|c| c.low))
            .push_constant("close", series(|c| c.close))
            .push_constant("volume", series(|c| c.volume))
            .push_constant("symbol", coin.symbol.to_string())
            .push_constant("inputs", inputs::to_script_map(input_values))
            .push_constant("fear_greed", fear_greed.unwrap_or(f64::NAN));

        let result = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &self.ast)
            .map_err(|e| format!("Error saat dijalankan: {e}"))?;
        parse_output(result)
    }
}

fn number(value: &Dynamic) -> Option<f64> {
    value.as_float().ok().or_else(|| value.as_int().ok().map(|v| v as f64))
}

/// Hasil script: angka, atau map `#{score, reason}` (dari `signal(...)`).
fn parse_output(value: Dynamic) -> Result<ScriptOutput, String> {
    let (score, reason, size_percent) = if let Some(score) = number(&value) {
        (score, String::new(), None)
    } else if let Some(map) = value.try_cast::<Map>() {
        let score = map.get("score").and_then(number).ok_or_else(|| {
            "signal() harus berisi skor angka".to_string()
        })?;
        let reason = map.get("reason").map(|r| r.to_string()).unwrap_or_default();
        let size = map.get("size").and_then(number).filter(|s| s.is_finite());
        (score, reason, size)
    } else {
        return Err("Script harus diakhiri skor angka 0–100 atau signal(skor, \"alasan\")".to_string());
    };
    if score.is_nan() {
        return Err("Skor NaN — data indikator belum cukup untuk coin ini".to_string());
    }
    Ok(ScriptOutput {
        score: score.clamp(0.0, 100.0),
        reason: reason.chars().take(200).collect(),
        size_percent: size_percent.map(|s| s.clamp(1.0, 100.0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rising(count: usize) -> Vec<Candle> {
        (0..count)
            .map(|i| {
                let close = 100.0 + i as f64;
                Candle { open: close - 0.5, high: close + 1.0, low: close - 1.0, close, volume: 1000.0 }
            })
            .collect()
    }

    fn run(source: &str, values: Value) -> Result<ScriptOutput, String> {
        let candles = rising(300);
        compile(source)?.run(&CoinData { symbol: "BTCUSDT", candles: &candles }, &values, Some(20.0))
    }

    #[test]
    fn contoh_di_dokumentasi_jalan() {
        let example = API_REFERENCE.split("CONTOH:\n").nth(1).unwrap().split("\n\nBATASAN").next().unwrap();
        let out = run(example, json!({"rsi_period": 14})).unwrap();
        // harga naik terus -> RSI 100, tidak murah
        assert_eq!(out, ScriptOutput { score: 0.0, reason: "belum cocok".to_string(), size_percent: None });
    }

    #[test]
    fn semua_fungsi_bisa_dipanggil() {
        let source = r#"
            let a = last(sma(close, 20)) + last(ema(close, 20)) + last(rsi(close, 14));
            let m = macd(close, 12, 26, 9); let b = bollinger(close, 20, 2.0);
            let s = stoch_rsi(close, 14, 14, 3, 3); let st = stochastic(high, low, close, 14, 3, 3);
            let x = last(atr(high, low, close, 14)) + last(adx(high, low, close, 14).adx);
            let v = last(vwap(high, low, close, volume, 20)) + last(obv(close, volume));
            let i = ichimoku(high, low, close, 9, 26, 52, 26); let t = supertrend(high, low, close, 10, 3);
            let ok = crossover(close, 50) || crossunder(close, sma(close, 5));
            let y = highest(close, 10) - lowest(close, 10) + change_percent(close, 5) + prev(close, 1);
            if is_na(fear_greed) { 0 } else { signal(last(t.uptrend) * 100, symbol) }
        "#;
        let out = run(source, json!({})).unwrap();
        assert_eq!(out, ScriptOutput { score: 100.0, reason: "BTCUSDT".to_string(), size_percent: None });
        assert_eq!(run("signal(80, \"x\", 25)", json!({})).unwrap().size_percent, Some(25.0));
        assert!(run("signal(80, \"x\", 150)", json!({})).is_err(), "size di luar 1–100 ditolak");
    }

    #[test]
    fn skor_dipotong_0_sampai_100() {
        assert_eq!(run("250", json!({})).unwrap().score, 100.0);
        assert_eq!(run("-5.5", json!({})).unwrap().score, 0.0);
    }

    #[test]
    fn script_salah_ditolak_dengan_pesan() {
        assert!(compile("let x = ;").unwrap_err().contains("Error sintaks"));
        assert!(run("\"teks\"", json!({})).unwrap_err().contains("skor angka"));
        assert!(run("fungsi_tidak_ada(close)", json!({})).unwrap_err().contains("Error saat dijalankan"));
        assert!(run("rsi(close, 0)", json!({})).is_err(), "periode 0 ditolak");
    }

    #[test]
    fn sandbox_menghentikan_loop_tanpa_akhir() {
        let err = run("loop { } 1", json!({})).unwrap_err();
        assert!(err.contains("Error saat dijalankan"), "{err}");
        assert!(compile("eval(\"1\")").is_err() || run("eval(\"1\")", json!({})).is_err());
    }
}
