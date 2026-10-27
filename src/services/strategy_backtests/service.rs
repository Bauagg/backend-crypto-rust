//! Backtest strategi: simulasi robot (aturan versi strategi) ke histori candle harian tersimpan,
//! lengkap dengan uji overfitting (`validation`) & analisa faktor (`insights`), hasilnya disimpan
//! (`strategy_backtests` + `strategy_backtest_trades`). Modal awal bisa diisi user.
//!
//! Backtest resmi (`is_official`, dipakai Store) = dijalankan pemilik strategi dengan modal
//! 1.000 USDT & setelan default — supaya semua strategi di Store dibandingkan dengan syarat sama.
//! User lain yang boleh memakai strategi bisa menjalankan backtest dengan modal & setelannya sendiri.

use std::sync::OnceLock;
use std::time::Instant;

use chrono::DateTime;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

use super::repository::{
    create_backtest, find_backtest_with_version, find_backtests_of_strategy, BacktestWithVersion, NewBacktest,
};
use super::types::{BacktestListQuery, BacktestView, RunBacktestInput};
use crate::database::{begin, commit, Tx};
use crate::services::candle_ohlcv::service::{load_daily_market, to_indicator_candles};
use crate::services::fear_greed::service::fear_greed_by_date;
use crate::services::strategies::model::Strategy;
use crate::services::strategies::repository::find_strategy_by_id;
use crate::services::store_listings::repository::find_listing_of_strategy;
use crate::services::strategies::service::usable_strategy_in_tx;
use crate::services::strategy_backtest_trades::model::StrategyBacktestTrade;
use crate::services::strategy_backtest_trades::repository::{create_backtest_trades, find_backtest_trades};
use crate::services::strategy_versions::repository::find_version_by_id;
use crate::services::strategy_versions::types::BotConfig;
use crate::strategy_engine::{self, inputs, run_backtest, BacktestConfig, BacktestOutput, SymbolHistory};
use crate::utils::app_error::AppError;

/// Modal awal default & modal backtest resmi (USDT).
const OFFICIAL_CAPITAL: f64 = 1000.0;
/// Batas modal yang boleh diisi user (USDT). Modal kecil tetap realistis: order < 5 USDT dilewati.
const MIN_CAPITAL: f64 = 10.0;
const MAX_CAPITAL: f64 = 1_000_000.0;
/// Fee spot Binance standar 0,1% per transaksi.
const FEE_RATE: f64 = 0.001;
/// Candle per coin yang diberikan ke script tiap hari simulasi.
const SCRIPT_WINDOW: usize = 300;
const MIN_CANDLES: usize = 30;
/// Lama simulasi maksimal (hari terakhir).
const PERIOD_DAYS: usize = 365;
/// Candle dibaca dari DB per coin: periode simulasi + pemanasan indikator.
const HISTORY_CANDLES: i64 = 800;
const BENCHMARK_SYMBOL: &str = "BTCUSDT";
/// Backtest berat (puluhan coin × ratusan hari) — maks sekian berjalan bersamaan di server.
const MAX_CONCURRENT: usize = 2;

fn permits() -> &'static Semaphore {
    static PERMITS: OnceLock<Semaphore> = OnceLock::new();
    PERMITS.get_or_init(|| Semaphore::new(MAX_CONCURRENT))
}

fn not_found() -> AppError {
    AppError::NotFound("Backtest tidak ditemukan".to_string())
}

fn dec(value: f64) -> Decimal {
    Decimal::from_f64(value).unwrap_or_default()
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn view(row: BacktestWithVersion, with_curve: bool) -> BacktestView {
    let equity_curve = with_curve.then(|| row.backtest.equity_curve.clone());
    BacktestView { backtest: row.backtest, strategy_id: row.strategy_id, version_no: row.version_no, equity_curve }
}

/// Modal awal dari input (default 1.000 USDT).
fn initial_capital(input: Option<Decimal>) -> Result<f64, AppError> {
    let Some(capital) = input else { return Ok(OFFICIAL_CAPITAL) };
    let capital: f64 = capital.try_into().unwrap_or(-1.0);
    if !(MIN_CAPITAL..=MAX_CAPITAL).contains(&capital) {
        return Err(AppError::BadRequest(format!(
            "initial_capital harus {MIN_CAPITAL}–{MAX_CAPITAL} USDT"
        )));
    }
    Ok(capital)
}

/// Jalankan backtest 1 versi strategi lalu simpan hasilnya.
pub async fn run_backtest_service(
    pool: &PgPool,
    user_id: Uuid,
    actor: &str,
    input: RunBacktestInput,
) -> Result<BacktestView, AppError> {
    let (strategy, version) = {
        let mut tx = begin(pool).await?;
        let (strategy, current) = usable_strategy_in_tx(&mut tx, input.strategy_id, user_id).await?;
        let version = match input.strategy_version_id {
            Some(id) if id != current.id => {
                // Versi lama hanya boleh diuji pemiliknya.
                if strategy.user_id != user_id {
                    return Err(AppError::BadRequest("Hanya versi aktif yang bisa diuji".to_string()));
                }
                find_version_by_id(&mut tx, id)
                    .await?
                    .filter(|v| v.strategy_id == strategy.id)
                    .ok_or_else(|| AppError::NotFound("Versi strategi tidak ditemukan".to_string()))?
            }
            _ => current,
        };
        commit(tx).await?;
        (strategy, version)
    };
    let capital = initial_capital(input.initial_capital)?;
    let specs = inputs::parse_specs(&version.inputs).map_err(AppError::BadRequest)?;
    let input_values = inputs::resolve_values(&specs, &input.input_values).map_err(AppError::BadRequest)?;
    let default_values = inputs::resolve_values(&specs, &Value::Null).map_err(AppError::BadRequest)?;
    let is_official = strategy.user_id == user_id && capital == OFFICIAL_CAPITAL && input_values == default_values;
    let bot = BotConfig::of(&version);

    let _permit = permits().try_acquire().map_err(|_| {
        AppError::BadRequest("Server sedang menjalankan backtest lain, coba lagi sebentar".to_string())
    })?;
    let started = Instant::now();

    let market = load_daily_market(pool, HISTORY_CANDLES).await?;
    let fear_greed = fear_greed_by_date(pool, HISTORY_CANDLES + 30).await?;
    let to_history = |symbol: &str, history: &[crate::services::candle_ohlcv::model::CandleOhlcv]| SymbolHistory {
        symbol: symbol.to_string(),
        dates: history
            .iter()
            .filter_map(|c| DateTime::from_timestamp_millis(c.open_time).map(|t| t.date_naive()))
            .collect(),
        candles: to_indicator_candles(history),
    };
    let scope = bot.scope();
    let histories: Vec<SymbolHistory> = market
        .fresh_coins()
        .filter(|(coin, _)| scope.is_none_or(|s| s.contains(&coin.value_param)))
        .map(|(coin, history)| to_history(&coin.value_param, history))
        .collect();
    if histories.is_empty() {
        return Err(AppError::BadRequest("Tidak ada data candle untuk coin strategi ini".to_string()));
    }
    let benchmark = market
        .fresh_coins()
        .find(|(coin, _)| coin.value_param == BENCHMARK_SYMBOL)
        .map(|(coin, history)| to_history(&coin.value_param, history));

    let config = BacktestConfig {
        entry_score: bot.entry_score.try_into().unwrap_or(70.0),
        exit_score: bot.exit_score.try_into().unwrap_or(30.0),
        position_percent: bot.position_percent.try_into().unwrap_or(20.0),
        max_positions: bot.max_positions.max(1) as usize,
        initial_capital: capital,
        fee_rate: FEE_RATE,
        window: SCRIPT_WINDOW,
        min_candles: MIN_CANDLES,
        period_days: PERIOD_DAYS,
    };
    let source = version.source_code.clone();
    let values = input_values.clone();
    let output: BacktestOutput = tokio::task::spawn_blocking(move || {
        let script = strategy_engine::compile(&source)?;
        run_backtest(&script, &histories, benchmark.as_ref(), &values, &specs, &fear_greed, &config)
    })
    .await
    .map_err(|err| {
        tracing::error!("Backtest panic/terhenti: {err}");
        AppError::Internal("Gagal menjalankan backtest".to_string())
    })?
    .map_err(AppError::BadRequest)?;

    let equity_curve: Vec<Value> = output
        .equity_curve
        .iter()
        .map(|p| {
            json!({
                "date": p.date,
                "equity": round2(p.equity),
                "drawdown_percent": round2(p.drawdown_percent),
                "btc_equity": p.btc_equity.map(round2),
            })
        })
        .collect();
    let equity_curve = Value::Array(equity_curve);
    let duration_ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;

    let mut tx = begin(pool).await?;
    let backtest = create_backtest(
        &mut tx,
        &NewBacktest {
            strategy_version_id: version.id,
            user_id,
            input_values: &input_values,
            period_start: output.period_start,
            period_end: output.period_end,
            initial_capital: dec(capital),
            fee_rate: dec(FEE_RATE),
            status: "DONE",
            stats: &output.stats,
            robustness: &output.robustness,
            equity_curve: &equity_curve,
            analysis: &output.analysis,
            is_official,
            validation: &output.validation,
            insights: &output.insights,
            error: None,
            duration_ms,
        },
        actor,
    )
    .await?;
    create_backtest_trades(&mut tx, backtest.id, &output.trades).await?;
    commit(tx).await?;

    let row = BacktestWithVersion { backtest, strategy_id: strategy.id, version_no: version.version_no };
    Ok(view(row, true))
}

/// Backtest yang boleh dilihat user: miliknya sendiri, atau milik pemilik strategi (resmi) kalau
/// user boleh memakai strategi itu — atau strategi sedang tampil di Store (calon pembeli boleh
/// melihat hasil backtest & daftar trade-nya sebelum membeli; isi script tetap tersembunyi).
async fn visible_backtest(tx: &mut Tx<'_>, id: Uuid, user_id: Uuid) -> Result<(BacktestWithVersion, Strategy), AppError> {
    let row = find_backtest_with_version(tx, id).await?.ok_or_else(not_found)?;
    let strategy = find_strategy_by_id(tx, row.strategy_id).await?.ok_or_else(not_found)?;
    let official = row.backtest.is_official;
    let allowed = row.backtest.user_id == user_id
        || strategy.user_id == user_id
        || (official && find_listing_of_strategy(tx, strategy.id).await?.is_some_and(|l| l.status == "ACTIVE"))
        || (official && usable_strategy_in_tx(tx, strategy.id, user_id).await.is_ok());
    if !allowed {
        return Err(not_found());
    }
    Ok((row, strategy))
}

/// Daftar backtest 1 strategi: punya user sendiri + resmi (pemilik strategi), terbaru dulu, tanpa
/// kurva modal.
pub async fn get_backtests_service(pool: &PgPool, user_id: Uuid, query: BacktestListQuery) -> Result<Vec<BacktestView>, AppError> {
    let mut tx = begin(pool).await?;
    let (strategy, _) = usable_strategy_in_tx(&mut tx, query.strategy_id, user_id).await?;
    let mut user_ids = vec![user_id];
    if strategy.user_id != user_id {
        user_ids.push(strategy.user_id);
    }
    let rows = find_backtests_of_strategy(&mut tx, strategy.id, &user_ids).await?;
    commit(tx).await?;
    // Backtest pemilik dengan modal/setelan custom hanya untuk pemiliknya sendiri.
    Ok(rows
        .into_iter()
        .filter(|row| row.backtest.user_id == user_id || row.backtest.is_official)
        .map(|row| view(row, false))
        .collect())
}

/// Detail backtest + kurva modal (equity, drawdown, pembanding BTC per hari).
pub async fn get_backtest_service(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<BacktestView, AppError> {
    let mut tx = begin(pool).await?;
    let (row, _) = visible_backtest(&mut tx, id, user_id).await?;
    commit(tx).await?;
    Ok(view(row, true))
}

/// Trade simulasi 1 backtest dengan pagination.
pub async fn get_backtest_trades_service(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<StrategyBacktestTrade>, i64), AppError> {
    let mut tx = begin(pool).await?;
    visible_backtest(&mut tx, id, user_id).await?;
    let page = find_backtest_trades(&mut tx, id, limit, offset).await?;
    commit(tx).await?;
    Ok(page)
}
