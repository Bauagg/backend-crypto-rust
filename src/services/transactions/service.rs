use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, Months, NaiveDate};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::model::{TradePosition, TradePositionWithPhoto};
use super::repository::{
    create_trade_position, create_trade_positions_bulk, find_open_positions_by_symbol,
    find_trade_position_by_id, find_trade_positions_for_dashboard, find_trade_positions_paginated,
};
use super::types::{
    AllocationItem, AssetStats, Dashboard, DashboardFilter, DashboardQuery, DashboardSummary,
    EquityPoint, Holding, HoldingBySymbolQuery, HoldingPosition, HoldingQuery, ManualSellResult,
    MonthlyPnl, NewTradePosition,
    TradeHighlight, TradeInput, TradePositionListQuery,
};
use crate::clients::binance;
use crate::database::{begin, commit};
use crate::robot::common;
use crate::websocket::TickerHub;
use crate::services::flex_params::repository::{find_flex_param_by_id, find_flex_param_by_type_and_value};
use crate::services::users::model::User;
use crate::services::users::repository::{credit_demo_balance, debit_demo_balance, find_user_by_id};
use crate::utils::app_error::AppError;
use crate::utils::crypto::decrypt;
use crate::utils::query_filter::{parse_filter_query, parse_sort_query, ColumnKind, FilterColumn};

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
const ACCOUNT_MODES: [&str; 2] = ["DEMO", "LIVE"];
const CURRENCIES: [&str; 2] = ["IDR", "USDT"];
const QUOTE_ASSET: &str = "USDT";
/// Pair Binance untuk kurs 1 USDT dalam Rupiah.
const USDT_IDR_SYMBOL: &str = "USDTIDR";

/// Kolom yang boleh dipakai di `?filter=` & `?sort=` list posisi. `user_id`/`user_email` sengaja
/// tidak ada — list selalu dikunci ke user yang login.
const TRADE_POSITION_COLUMNS: [FilterColumn; 28] = [
    FilterColumn::new("id", ColumnKind::Uuid),
    FilterColumn::new("flex_param_id", ColumnKind::Uuid),
    FilterColumn::new("symbol", ColumnKind::Text),
    FilterColumn::new("source", ColumnKind::Text),
    FilterColumn::new("strategy", ColumnKind::Text),
    FilterColumn::new("opened_at", ColumnKind::Timestamp),
    FilterColumn::new("closed_at", ColumnKind::Timestamp),
    FilterColumn::new("holding_days", ColumnKind::Integer),
    FilterColumn::new("currency", ColumnKind::Text),
    FilterColumn::new("usdt_idr_rate", ColumnKind::Numeric),
    FilterColumn::new("quantity", ColumnKind::Numeric),
    FilterColumn::new("entry_value", ColumnKind::Numeric),
    FilterColumn::new("exit_value", ColumnKind::Numeric),
    FilterColumn::new("entry_price", ColumnKind::Numeric),
    FilterColumn::new("exit_price", ColumnKind::Numeric),
    FilterColumn::new("fee_amount", ColumnKind::Numeric),
    FilterColumn::new("entry_order_id", ColumnKind::Text),
    FilterColumn::new("exit_order_id", ColumnKind::Text),
    FilterColumn::new("pnl_amount", ColumnKind::Numeric),
    FilterColumn::new("pnl_percent", ColumnKind::Numeric),
    FilterColumn::new("status", ColumnKind::Text),
    FilterColumn::new("result", ColumnKind::Text),
    FilterColumn::new("account_capital", ColumnKind::Numeric),
    FilterColumn::new("account_mode", ColumnKind::Text),
    FilterColumn::new("created_at", ColumnKind::Timestamp),
    FilterColumn::new("created_by", ColumnKind::Text),
    FilterColumn::new("updated_at", ColumnKind::Timestamp),
    FilterColumn::new("updated_by", ColumnKind::Text),
];

/// List posisi milik user yang login, terbaru dulu kalau tidak ada `sort`.
pub async fn get_trade_positions_service(
    pool: &PgPool,
    user_id: Uuid,
    query: &TradePositionListQuery,
    limit: i64,
    offset: i64,
) -> Result<(Vec<TradePositionWithPhoto>, i64), AppError> {
    let search = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let filters = parse_filter_query(query.filter.as_deref())?;
    let sort = parse_sort_query(query.sort.as_deref(), query.order.as_deref())?;

    let mut tx = begin(pool).await?;
    let page = find_trade_positions_paginated(
        &mut tx,
        user_id,
        search,
        &filters,
        sort.as_ref(),
        &TRADE_POSITION_COLUMNS,
        "opened_at DESC",
        limit,
        offset,
    )
    .await?;
    commit(tx).await?;
    Ok(page)
}

/// Detail 1 posisi milik user yang login. Posisi milik user lain dibalas 404 (sama seperti tidak ada).
pub async fn get_trade_position_by_id_service(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> Result<TradePositionWithPhoto, AppError> {
    let mut tx = begin(pool).await?;
    let position = find_trade_position_by_id(&mut tx, id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Posisi trading tidak ditemukan".to_string()))?;
    commit(tx).await?;
    Ok(position)
}

/// Saldo akun LIVE (USDT): `capital` = total nilai akun (USDT + semua aset lain, dicatat sebagai
/// `account_capital`), `free_usdt` = USDT bebas yang benar-benar bisa dipakai membeli.
struct LiveBalance {
    capital: Decimal,
    free_usdt: Decimal,
}

/// Coin dihitung lewat pair `<ASET>USDT` (qty × harga); mata uang fiat seperti IDR lewat pair
/// kebalikannya `USDT<ASET>` (qty ÷ kurs). Aset tanpa pair ke USDT sama sekali (mis. token Earn
/// `LD...`) tidak dihitung. Harga dari exchange tempat akun berada (testnet / Binance asli).
async fn live_balance(api_key: &str, api_secret: &str) -> Result<LiveBalance, AppError> {
    let balances = binance::get_account_balances(api_key, api_secret).await?;
    let prices = binance::get_account_prices().await?;

    let mut capital = Decimal::ZERO;
    let mut free_usdt = Decimal::ZERO;
    for balance in balances {
        let amount = balance.free + balance.locked;
        if balance.asset == QUOTE_ASSET {
            capital += amount;
            free_usdt += balance.free;
        } else if let Some(price) = prices.get(&format!("{}{QUOTE_ASSET}", balance.asset)) {
            capital += amount * price;
        } else if let Some(rate) = prices
            .get(&format!("{QUOTE_ASSET}{}", balance.asset))
            .filter(|rate| !rate.is_zero())
        {
            capital += amount / rate;
        }
    }
    Ok(LiveBalance {
        capital: capital.round_dp(8),
        free_usdt: free_usdt.round_dp(8),
    })
}

// ---------------------------------------------------------------------------------------------
// BUY & SELL manual — dieksekusi seperti robot: DEMO disimulasikan dengan hitungan Binance (harga
// pasar, qty dibulatkan ke stepSize, minimum order, fee 0,1%) dan memotong/menambah `demo_balance`;
// LIVE mengirim order MARKET asli ke Binance (atau testnet, `ACCOUNT_API_BASE_URL`) dan mencatat
// hasil eksekusi + bukti order. Posisi manual (`source = MANUAL`) tidak pernah disentuh robot.
// ---------------------------------------------------------------------------------------------

const MANUAL_SOURCE: &str = "MANUAL";

fn rule_error(symbol: &str) -> AppError {
    AppError::BadRequest(format!("Aturan order {symbol} tidak tersedia di exchange"))
}

/// Order MARKET wajib lolos semua filter Binance pair itu (DEMO & LIVE sama) — batasnya beda per
/// coin, mis. minimal 5 USDT untuk BTC, 1 USDT untuk DOGE.
fn check_order(
    rule: &binance::TradingRules,
    side: &str,
    symbol: &str,
    quantity: Decimal,
    notional: Decimal,
) -> Result<(), AppError> {
    let coin = common::base_asset(symbol);
    let message = match rule.market_violation(quantity, notional) {
        None => return Ok(()),
        Some(binance::RuleViolation::MinQty(min)) => {
            format!("Jumlah {side} minimal {} {coin} untuk {symbol}", min.normalize())
        }
        Some(binance::RuleViolation::MaxQty(max)) => {
            format!("Jumlah {side} maksimal {} {coin} per order untuk {symbol}", max.normalize())
        }
        Some(binance::RuleViolation::MinNotional(min)) => {
            format!("Nilai {side} minimal {} USDT untuk {symbol}", min.normalize())
        }
        Some(binance::RuleViolation::MaxNotional(max)) => {
            format!("Nilai {side} maksimal {} USDT per order untuk {symbol}", max.normalize())
        }
    };
    Err(AppError::BadRequest(message))
}

fn credentials_of(user: &User) -> Result<(String, String), AppError> {
    let (Some(key), Some(secret)) = (&user.api_key, &user.api_secret) else {
        return Err(AppError::BadRequest(
            "Akun LIVE butuh API key & secret Binance — isi dulu di profil".to_string(),
        ));
    };
    Ok((decrypt(key)?, decrypt(secret)?))
}

/// Harga untuk konversi fee ke USDT — hanya diambil kalau ada fee yang dibayar selain coin/USDT
/// (mis. BNB), supaya tidak setiap order membebani request harga semua pair.
async fn fee_prices(fills: &[binance::OrderFill], symbol: &str) -> HashMap<String, Decimal> {
    let base = common::base_asset(symbol);
    if fills.iter().all(|f| f.commission_asset == QUOTE_ASSET || f.commission_asset == base) {
        return HashMap::new();
    }
    binance::get_account_prices().await.unwrap_or_default()
}

/// Order yang sudah terisi di exchange tapi gagal dicatat — jangan hilang: tulis lengkap ke log.
fn unrecorded_order(user_email: &str, symbol: &str, raw: &serde_json::Value, err: AppError) -> AppError {
    tracing::error!(
        "Transaksi manual {user_email}: order {symbol} SUDAH TERISI di Binance tapi gagal dicatat \
         ({err:?}) — cocokkan manual: {raw}"
    );
    AppError::Internal(format!(
        "Order {symbol} sudah terisi di Binance tapi gagal dicatat. Hubungi admin untuk dicocokkan"
    ))
}

async fn market_rule(symbol: &str) -> Result<binance::TradingRules, AppError> {
    binance::get_trading_rules(&[symbol.to_string()])
        .await?
        .get(symbol)
        .copied()
        .ok_or_else(|| rule_error(symbol))
}

async fn account_rule(symbol: &str) -> Result<binance::TradingRules, AppError> {
    binance::get_account_trading_rules(&[symbol.to_string()])
        .await?
        .get(symbol)
        .copied()
        .ok_or_else(|| rule_error(symbol))
}

async fn market_price(symbol: &str) -> Result<Decimal, AppError> {
    binance::get_price(symbol)
        .await
        .map_err(|_| AppError::BadRequest(format!("Gagal mengambil harga {symbol} dari Binance")))
}

/// Harga dari exchange tempat order LIVE dikirim (testnet / Binance asli).
async fn account_price(symbol: &str) -> Result<Decimal, AppError> {
    binance::get_account_prices()
        .await?
        .get(symbol)
        .copied()
        .ok_or_else(|| AppError::BadRequest(format!("Harga {symbol} tidak tersedia di exchange")))
}

/// Hasil `POST /api/transactions`: BUY = posisi baru, SELL = ringkasan penjualan.
pub enum TradeOutcome {
    Bought(Box<TradePositionWithPhoto>),
    Sold(Box<ManualSellResult>),
}

/// BUY / SELL manual dari 1 endpoint, dipilih `transaction_type`.
pub async fn trade_service(
    pool: &PgPool,
    user_id: Uuid,
    user_email: &str,
    input: TradeInput,
) -> Result<TradeOutcome, AppError> {
    match input.transaction_type.trim().to_uppercase().as_str() {
        "BUY" => Ok(TradeOutcome::Bought(
            Box::new(create_trade_position_service(pool, user_id, user_email, input).await?),
        )),
        "SELL" => Ok(TradeOutcome::Sold(
            Box::new(sell_trade_position_service(pool, user_id, user_email, input).await?),
        )),
        _ => Err(AppError::BadRequest("transaction_type harus BUY atau SELL".to_string())),
    }
}

/// BUY manual (buka posisi) senilai `amount`. `amount` dalam IDR dikonversi ke USDT pakai kurs
/// saat itu; kurs ikut disimpan supaya nilai posisi bisa dikonversi ke Rupiah kapan pun.
///
/// Urutan: baca user & coin (transaksi singkat) -> harga/aturan order/eksekusi di exchange (tanpa
/// transaksi) -> simpan posisi (transaksi singkat).
async fn create_trade_position_service(
    pool: &PgPool,
    user_id: Uuid,
    user_email: &str,
    input: TradeInput,
) -> Result<TradePositionWithPhoto, AppError> {
    let account_mode = parse_account_mode(Some(&input.account_mode))?;
    if input.amount <= Decimal::ZERO {
        return Err(AppError::BadRequest("amount harus lebih dari 0".to_string()));
    }

    let (user, coin) = {
        let mut tx = begin(pool).await?;
        let user = find_user_by_id(&mut tx, user_id).await?;
        let coin = find_flex_param_by_id(&mut tx, input.flex_param_id).await?;
        commit(tx).await?;
        (user, coin)
    };
    let user = user.ok_or_else(|| AppError::Unauthorized("User tidak ditemukan".to_string()))?;

    // Tidak dikirim FE -> pakai mata uang pilihan user di profil.
    let currency = input
        .currency
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or(&user.preferred_currency)
        .to_uppercase();
    if !CURRENCIES.contains(&currency.as_str()) {
        return Err(AppError::BadRequest("currency harus IDR atau USDT".to_string()));
    }

    let coin = coin
        .filter(|p| p.type_param == SYMBOL_TYPE_PARAM)
        .ok_or_else(|| AppError::NotFound("Coin tidak ditemukan".to_string()))?;
    if !coin.is_active {
        return Err(AppError::BadRequest(format!("Coin {} sedang tidak aktif", coin.value_param)));
    }
    let symbol = coin.value_param.clone();

    let usdt_idr_rate = binance::get_price(USDT_IDR_SYMBOL).await.map_err(|_| {
        AppError::BadRequest("Gagal mengambil kurs USDT/IDR dari Binance".to_string())
    })?;
    let entry_value = match currency.as_str() {
        "IDR" => (input.amount / usdt_idr_rate).round_dp(8),
        _ => input.amount,
    };

    let new_row = |quantity: Decimal, cost: Decimal, price: Decimal, fee: Decimal, capital: Decimal| {
        NewTradePosition {
            user_id,
            user_email: user_email.to_string(),
            flex_param_id: coin.id,
            symbol: symbol.clone(),
            source: MANUAL_SOURCE.to_string(),
            strategy: None,
            opened_at: chrono::Utc::now(),
            closed_at: None,
            holding_days: 0,
            currency: currency.clone(),
            usdt_idr_rate,
            quantity: quantity.round_dp(8),
            entry_value: cost.round_dp(8),
            exit_value: None,
            entry_price: price.round_dp(8),
            exit_price: None,
            fee_amount: fee.round_dp(8),
            entry_order_id: None,
            exit_order_id: None,
            entry_order_response: None,
            exit_order_response: None,
            pnl_amount: Decimal::ZERO,
            pnl_percent: Decimal::ZERO,
            status: "OPEN".to_string(),
            result: None,
            account_capital: capital.round_dp(8),
            account_mode: account_mode.clone(),
            created_by: user_email.to_string(),
        }
    };

    let position = if account_mode == "DEMO" {
        // Simulasi seperti robot demo: qty dibulatkan ke stepSize, fee 0,1% dipotong dari coin.
        if entry_value > user.demo_balance {
            return Err(AppError::BadRequest(format!(
                "Saldo demo tidak cukup ({} USDT)",
                user.demo_balance.round_dp(2)
            )));
        }
        let price = market_price(&symbol).await?;
        let rule = market_rule(&symbol).await?;
        let quantity = common::floor_to_step(entry_value / price, rule.step_size);
        let cost = quantity * price;
        check_order(&rule, "beli", &symbol, quantity, cost)?;
        let fee_quantity = quantity * common::fee_rate();
        let row = new_row(quantity - fee_quantity, cost, price, fee_quantity * price, user.demo_balance);

        let mut tx = begin(pool).await?;
        if !debit_demo_balance(&mut tx, user_id, cost.round_dp(8)).await? {
            return Err(AppError::BadRequest("Saldo demo tidak cukup".to_string()));
        }
        let position = create_trade_position(&mut tx, &row).await?;
        commit(tx).await?;
        position
    } else {
        // Order MARKET asli senilai entry_value USDT (quoteOrderQty), dicatat dari hasil eksekusi.
        // Membeli hanya bisa memakai USDT bebas — bukan total akun (coin lain tidak ikut terpakai).
        let (api_key, api_secret) = credentials_of(&user)?;
        let balance = live_balance(&api_key, &api_secret).await?;
        if entry_value > balance.free_usdt {
            return Err(AppError::BadRequest(format!(
                "Saldo USDT di Binance tidak cukup (tersedia {} USDT)",
                balance.free_usdt.round_dp(2)
            )));
        }
        let account_capital = balance.capital;
        let rule = account_rule(&symbol).await?;
        let spend = entry_value.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::ToZero);
        // quoteOrderQty: Binance menghitung qty dari harga saat itu — perkiraan qty yang sama
        // dicek di sini supaya order yang pasti ditolak tidak dikirim.
        let price = account_price(&symbol).await?;
        let estimated = common::floor_to_step(spend / price, rule.step_size);
        check_order(&rule, "beli", &symbol, estimated, spend)?;

        let (result, raw) = binance::place_market_order(
            &api_key,
            &api_secret,
            &symbol,
            "BUY",
            binance::OrderAmount::Quote(spend),
        )
        .await?;
        let price = common::average_price(&result);
        let prices = fee_prices(&result.fills, &symbol).await;
        let quantity = result.executed_qty
            - common::commission_in(&result.fills, common::base_asset(&symbol));
        let fee = common::fee_in_usdt(&result.fills, &symbol, price, &prices);
        let mut row = new_row(quantity, result.quote_qty, price, fee, account_capital);
        row.entry_order_id = Some(result.order_id.to_string());
        row.entry_order_response = Some(raw.clone());

        let recorded = async {
            let mut tx = begin(pool).await?;
            let position = create_trade_position(&mut tx, &row).await?;
            commit(tx).await?;
            Ok::<_, AppError>(position)
        }
        .await;
        recorded.map_err(|err| unrecorded_order(user_email, &symbol, &raw, err))?
    };

    Ok(TradePositionWithPhoto {
        position,
        photo_url: coin.photo_url,
    })
}

/// Coin yang sedang dijual per user & mode akun — supaya klik ganda / 2 request bersamaan tidak
/// mengirim 2 order SELL untuk coin yang sama (berlaku dalam 1 instance server).
static SELLING: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

struct SellingGuard(String);

impl SellingGuard {
    fn acquire(key: String) -> Result<Self, AppError> {
        let set = SELLING.get_or_init(Default::default);
        if !set.lock().expect("lock jual tidak boleh poisoned").insert(key.clone()) {
            return Err(AppError::Conflict("Penjualan coin ini sedang diproses, tunggu sebentar".to_string()));
        }
        Ok(Self(key))
    }
}

impl Drop for SellingGuard {
    fn drop(&mut self) {
        if let Some(set) = SELLING.get() {
            set.lock().expect("lock jual tidak boleh poisoned").remove(&self.0);
        }
    }
}

fn parse_account_mode(value: Option<&str>) -> Result<String, AppError> {
    let mode = value.unwrap_or("DEMO").trim().to_uppercase();
    if !ACCOUNT_MODES.contains(&mode.as_str()) {
        return Err(AppError::BadRequest("account_mode harus DEMO atau LIVE".to_string()));
    }
    Ok(mode)
}

/// Coin dicari lewat id `flex_params` atau simbolnya.
enum CoinRef<'a> {
    Id(Uuid),
    /// `BTC` / `btcusdt` / `BTCUSDT` — tanpa `USDT` otomatis ditambahkan.
    Symbol(&'a str),
}

/// Coin (SIMBOL_CRYPTO) + SEMUA posisi OPEN user untuk coin itu (manual maupun robot), terlama
/// dulu (FIFO) — total inilah batas maksimal yang boleh dijual.
async fn owned_positions_of(
    pool: &PgPool,
    user_id: Uuid,
    coin: CoinRef<'_>,
    account_mode: &str,
) -> Result<(Option<User>, crate::services::flex_params::model::FlexParam, Vec<TradePosition>), AppError> {
    let mut tx = begin(pool).await?;
    let user = find_user_by_id(&mut tx, user_id).await?;
    let coin = match coin {
        CoinRef::Id(id) => find_flex_param_by_id(&mut tx, id).await?,
        CoinRef::Symbol(symbol) => {
            let symbol = symbol.trim().to_uppercase();
            let symbol = if symbol.ends_with(QUOTE_ASSET) { symbol } else { format!("{symbol}{QUOTE_ASSET}") };
            find_flex_param_by_type_and_value(&mut tx, SYMBOL_TYPE_PARAM, &symbol, None).await?
        }
    }
    .filter(|p| p.type_param == SYMBOL_TYPE_PARAM)
    .ok_or_else(|| AppError::NotFound("Coin tidak ditemukan".to_string()))?;
    let positions = find_open_positions_by_symbol(&mut tx, user_id, account_mode, &coin.value_param).await?;
    commit(tx).await?;
    Ok((user, coin, positions))
}

/// Modal USDT yang dipakai untuk `quantity` coin pertama dari posisi FIFO — dasar untung/rugi
/// bagian yang dijual (sama dengan yang dicatat `close_positions_fifo`).
fn fifo_entry_value(positions: &[TradePosition], quantity: Decimal) -> Decimal {
    let mut remaining = quantity;
    let mut entry = Decimal::ZERO;
    for position in positions.iter().filter(|p| !p.quantity.is_zero()) {
        if remaining.is_zero() {
            break;
        }
        let take = remaining.min(position.quantity);
        entry += position.entry_value * (take / position.quantity);
        remaining -= take;
    }
    entry
}

/// Total 1 coin yang dimiliki user (posisi OPEN manual + robot), dinilai dengan harga sekarang —
/// untuk layar Sell ("Dimiliki: 250 ADA ≈ $54").
pub async fn get_holding_service(
    pool: &PgPool,
    user_id: Uuid,
    query: HoldingQuery,
) -> Result<Holding, AppError> {
    let account_mode = parse_account_mode(query.account_mode.as_deref())?;
    holding_of(pool, user_id, CoinRef::Id(query.flex_param_id), account_mode).await
}

/// Seperti `get_holding_service`, coin dicari dari simbolnya (`BTC` / `BTCUSDT`) — detail saldo
/// 1 coin: total dari semua posisi OPEN + rincian tiap posisinya.
pub async fn get_holding_by_symbol_service(
    pool: &PgPool,
    user_id: Uuid,
    symbol: &str,
    query: HoldingBySymbolQuery,
) -> Result<Holding, AppError> {
    let account_mode = parse_account_mode(query.account_mode.as_deref())?;
    holding_of(pool, user_id, CoinRef::Symbol(symbol), account_mode).await
}

async fn holding_of(
    pool: &PgPool,
    user_id: Uuid,
    coin: CoinRef<'_>,
    account_mode: String,
) -> Result<Holding, AppError> {
    let (user, coin, positions) = owned_positions_of(pool, user_id, coin, &account_mode).await?;
    let user = user.ok_or_else(|| AppError::Unauthorized("User tidak ditemukan".to_string()))?;
    let available_balance = if account_mode == "DEMO" {
        Some(user.demo_balance.round_dp(8))
    } else {
        match credentials_of(&user) {
            Ok((api_key, api_secret)) => {
                Some(live_balance(&api_key, &api_secret).await?.free_usdt.round_dp(8))
            }
            Err(_) => None,
        }
    };

    let quantity: Decimal = positions.iter().map(|p| p.quantity).sum();
    let manual_quantity: Decimal =
        positions.iter().filter(|p| p.source == MANUAL_SOURCE).map(|p| p.quantity).sum();
    let entry_value: Decimal = positions.iter().map(|p| p.entry_value).sum();
    let current_price = if quantity.is_zero() {
        Decimal::ZERO
    } else {
        market_price(&coin.value_param).await?
    };
    let current_value = quantity * current_price;
    let unrealized = current_value - entry_value;
    let now = chrono::Utc::now();
    let details = positions
        .iter()
        .map(|p| {
            let value = p.quantity * current_price;
            let pnl = value - p.entry_value;
            HoldingPosition {
                id: p.id,
                source: p.source.clone(),
                strategy: p.strategy.clone(),
                opened_at: p.opened_at,
                holding_days: (now - p.opened_at).num_days(),
                quantity: p.quantity.round_dp(8),
                entry_price: p.entry_price,
                entry_value: p.entry_value.round_dp(8),
                current_value: value.round_dp(8),
                unrealized_pnl: pnl.round_dp(8),
                unrealized_pnl_percent: common::percent(pnl, p.entry_value),
            }
        })
        .collect();

    Ok(Holding {
        flex_param_id: coin.id,
        symbol: coin.value_param,
        photo_url: coin.photo_url,
        account_mode,
        quantity: quantity.round_dp(8),
        manual_quantity: manual_quantity.round_dp(8),
        bot_quantity: (quantity - manual_quantity).round_dp(8),
        entry_value: entry_value.round_dp(8),
        avg_entry_price: if quantity.is_zero() { Decimal::ZERO } else { (entry_value / quantity).round_dp(8) },
        current_price,
        current_value: current_value.round_dp(8),
        unrealized_pnl: unrealized.round_dp(8),
        unrealized_pnl_percent: common::percent(unrealized, entry_value),
        open_positions: positions.len(),
        available_balance,
        positions: details,
    })
}

/// SELL manual 1 coin senilai `amount` — seperti robot: SEMUA posisi OPEN coin itu (manual maupun
/// robot) dijual FIFO (terlama dulu), maksimal sebanyak yang dimiliki; posisi yang hanya terjual
/// sebagian dipecah jadi baris CLOSED + sisa tetap OPEN. DEMO: simulasi harga pasar + fee 0,1% dari
/// USDT, hasil bersih masuk `demo_balance`. LIVE: order MARKET asli.
async fn sell_trade_position_service(
    pool: &PgPool,
    user_id: Uuid,
    user_email: &str,
    input: TradeInput,
) -> Result<ManualSellResult, AppError> {
    let account_mode = parse_account_mode(Some(&input.account_mode))?;
    if input.amount <= Decimal::ZERO {
        return Err(AppError::BadRequest("amount harus lebih dari 0".to_string()));
    }
    let _guard = SellingGuard::acquire(format!("{user_id}:{account_mode}:{}", input.flex_param_id))?;

    let (user, coin, mut positions) =
        owned_positions_of(pool, user_id, CoinRef::Id(input.flex_param_id), &account_mode).await?;
    let user = user.ok_or_else(|| AppError::Unauthorized("User tidak ditemukan".to_string()))?;
    let symbol = coin.value_param.clone();
    let held: Decimal = positions.iter().map(|p| p.quantity).sum();
    if held.is_zero() {
        return Err(AppError::BadRequest(format!(
            "Anda tidak memegang {symbol} di akun {account_mode}"
        )));
    }

    let price = if account_mode == "DEMO" {
        market_price(&symbol).await?
    } else {
        account_price(&symbol).await?
    };

    // Jumlah coin yang diminta = `amount` (USDT/IDR) / harga, maksimal yang dipegang — `amount`
    // senilai atau lebih dari semua coin = jual semua.
    let currency = input
        .currency
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or(&user.preferred_currency)
        .to_uppercase();
    let amount_usdt = match currency.as_str() {
        "USDT" => input.amount,
        "IDR" => input.amount / binance::get_price(USDT_IDR_SYMBOL).await.map_err(|_| {
            AppError::BadRequest("Gagal mengambil kurs USDT/IDR dari Binance".to_string())
        })?,
        _ => return Err(AppError::BadRequest("currency harus IDR atau USDT".to_string())),
    };
    let wanted = (amount_usdt / price).min(held);
    let sell_everything = wanted >= held;
    let now = chrono::Utc::now();

    let fill = if account_mode == "DEMO" {
        let rule = market_rule(&symbol).await?;
        // Jual semua = seluruh qty (seperti robot `jual_semua`, supaya tidak tersisa debu dari fee
        // 0,1% saat beli); sebagian = dibulatkan ke stepSize. Keduanya tetap wajib lolos filter Binance.
        let quantity = if sell_everything {
            held
        } else {
            common::floor_to_step(wanted, rule.step_size)
        };
        let proceeds = quantity * price;
        check_order(&rule, "jual", &symbol, quantity, proceeds)?;
        let fee = proceeds * common::fee_rate();
        common::SellFill {
            symbol: symbol.clone(),
            quantity,
            net_total: proceeds - fee,
            fee_total: fee,
            price,
            at: now,
            order_id: None,
            order_response: None,
        }
    } else {
        let (api_key, api_secret) = credentials_of(&user)?;
        let rule = account_rule(&symbol).await?;
        let base = common::base_asset(&symbol);
        let available: Decimal = binance::get_account_balances(&api_key, &api_secret)
            .await?
            .into_iter()
            .filter(|b| b.asset == base)
            .map(|b| b.free)
            .sum();
        // Tidak menjual lebih dari yang benar-benar ada di Binance (mis. sebagian sudah dijual user
        // lewat aplikasi Binance), lalu dibulatkan ke stepSize.
        let quantity = common::floor_to_step(wanted.min(available), rule.step_size);
        check_order(&rule, "jual", &symbol, quantity, quantity * price)?;

        let (result, raw) = binance::place_market_order(
            &api_key,
            &api_secret,
            &symbol,
            "SELL",
            binance::OrderAmount::Base(quantity),
        )
        .await?;
        let avg_price = common::average_price(&result);
        let prices = fee_prices(&result.fills, &symbol).await;
        common::SellFill {
            symbol: symbol.clone(),
            quantity: result.executed_qty,
            net_total: result.quote_qty - common::commission_in(&result.fills, QUOTE_ASSET),
            fee_total: common::fee_in_usdt(&result.fills, &symbol, avg_price, &prices),
            price: avg_price.round_dp(8),
            at: now,
            order_id: Some(result.order_id.to_string()),
            order_response: Some(raw),
        }
    };

    // Dihitung sebelum posisi diubah oleh close_positions_fifo.
    let entry_part = fifo_entry_value(&positions, fill.quantity);
    let positions_sold = {
        let mut remaining = fill.quantity;
        positions
            .iter()
            .take_while(|p| {
                let touched = !remaining.is_zero();
                remaining -= remaining.min(p.quantity);
                touched
            })
            .count()
    };

    let recorded = async {
        let mut tx = begin(pool).await?;
        let mut new_rows = Vec::new();
        common::close_positions_fifo(&mut tx, &mut positions, &fill, &mut new_rows).await?;
        create_trade_positions_bulk(&mut tx, &new_rows).await?;
        if account_mode == "DEMO" {
            credit_demo_balance(&mut tx, user_id, fill.net_total.round_dp(8)).await?;
        }
        commit(tx).await
    }
    .await;
    if let Err(err) = recorded {
        return Err(match &fill.order_response {
            Some(raw) => unrecorded_order(user_email, &symbol, raw, err),
            None => err,
        });
    }

    let pnl = fill.net_total - entry_part;
    Ok(ManualSellResult {
        symbol,
        account_mode,
        sold_quantity: fill.quantity.round_dp(8),
        exit_price: fill.price.round_dp(8),
        exit_value: fill.net_total.round_dp(8),
        fee_amount: fill.fee_total.round_dp(8),
        pnl_amount: pnl.round_dp(8),
        pnl_percent: common::percent(pnl, entry_part),
        result: common::result_of(pnl),
        positions_sold,
        remaining_quantity: (held - fill.quantity).max(Decimal::ZERO).round_dp(8),
        order_id: fill.order_id.clone(),
    })
}

// ---------------------------------------------------------------------------------------------
// Dashboard — performa trading 1 user per mode akun, bentuknya mengikuti laporan backtest Python
// supaya hasil nyata bisa dibandingkan dengan simulasinya. Posisi OPEN dinilai dengan harga live.
// ---------------------------------------------------------------------------------------------

const SOURCES: [&str; 2] = ["BOT", "MANUAL"];

fn round_money(value: Decimal) -> Decimal {
    value.round_dp(4)
}

fn ratio_percent(part: Decimal, whole: Decimal) -> Decimal {
    if whole.is_zero() {
        Decimal::ZERO
    } else {
        (part / whole * Decimal::ONE_HUNDRED).round_dp(2)
    }
}

fn average(sum: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        sum / Decimal::from(count)
    }
}

fn closed_date(p: &TradePositionWithPhoto) -> Option<NaiveDate> {
    p.position.closed_at.map(|t| t.date_naive())
}

fn highlight(p: &TradePositionWithPhoto) -> TradeHighlight {
    TradeHighlight {
        id: p.position.id,
        symbol: p.position.symbol.clone(),
        pnl_amount: round_money(p.position.pnl_amount),
        pnl_percent: p.position.pnl_percent.round_dp(2),
        closed_at: p.position.closed_at,
    }
}

/// Hitung seluruh isi dashboard dari daftar posisi — fungsi murni, tanpa I/O. `prices` = harga
/// sekarang per simbol; simbol tanpa harga dinilai dengan harga masuknya.
pub fn build_dashboard(
    filter: DashboardFilter,
    positions: &[TradePositionWithPhoto],
    prices: &HashMap<String, Decimal>,
    cash: Option<Decimal>,
    initial_capital: Option<Decimal>,
    usdt_idr_rate: Option<Decimal>,
    preferred_currency: String,
) -> Dashboard {
    let in_range = |date: NaiveDate| {
        filter.date_from.is_none_or(|from| date >= from) && filter.date_to.is_none_or(|to| date <= to)
    };
    let initial_capital =
        initial_capital.or_else(|| positions.first().map(|p| p.position.account_capital));

    // Kurva modal & bulanan dihitung dari SELURUH riwayat (modal awal bulan butuh hasil sebelumnya),
    // yang ditampilkan hanya yang masuk rentang filter.
    let mut closed: Vec<&TradePositionWithPhoto> =
        positions.iter().filter(|p| p.position.status == "CLOSED").collect();
    closed.sort_by_key(|p| p.position.closed_at);

    let mut daily: BTreeMap<NaiveDate, Decimal> = BTreeMap::new();
    let mut monthly_raw: BTreeMap<String, (Decimal, usize, usize)> = BTreeMap::new();
    for p in &closed {
        let Some(date) = closed_date(p) else { continue };
        *daily.entry(date).or_default() += p.position.pnl_amount;
        let month = monthly_raw.entry(date.format("%Y-%m").to_string()).or_default();
        month.0 += p.position.pnl_amount;
        month.1 += 1;
        if p.position.pnl_amount >= Decimal::ZERO {
            month.2 += 1;
        }
    }

    let mut equity = Vec::new();
    let mut cumulative = Decimal::ZERO;
    let mut peak = initial_capital;
    let mut max_drawdown = Decimal::ZERO;
    for (date, pnl) in &daily {
        cumulative += pnl;
        let value = initial_capital.map(|c| c + cumulative);
        let drawdown = match (value, peak) {
            (Some(v), Some(pk)) => {
                let pk = pk.max(v);
                peak = Some(pk);
                if pk.is_zero() { Decimal::ZERO } else { ((v - pk) / pk * Decimal::ONE_HUNDRED).round_dp(2) }
            }
            _ => Decimal::ZERO,
        };
        if in_range(*date) {
            max_drawdown = max_drawdown.min(drawdown);
            equity.push(EquityPoint {
                date: *date,
                realized_pnl: round_money(*pnl),
                cumulative_pnl: round_money(cumulative),
                equity: value.map(round_money),
                drawdown_percent: drawdown,
            });
        }
    }

    let mut monthly = Vec::new();
    let mut before_month = Decimal::ZERO;
    for (month, (pnl, trades, wins)) in &monthly_raw {
        let start_equity = initial_capital.map(|c| c + before_month);
        before_month += pnl;
        let first_day = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok();
        let visible = first_day.is_some_and(|d| {
            let last_day = d + Months::new(1) - Duration::days(1);
            filter.date_from.is_none_or(|from| last_day >= from) && filter.date_to.is_none_or(|to| d <= to)
        });
        if visible {
            monthly.push(MonthlyPnl {
                month: month.clone(),
                pnl: round_money(*pnl),
                pnl_percent: start_equity.filter(|e| !e.is_zero()).map(|e| ratio_percent(*pnl, e)),
                trades: *trades,
                wins: *wins,
            });
        }
    }

    // Statistik trade: posisi CLOSED dalam rentang + semua posisi OPEN (kondisi sekarang).
    let closed_in_range: Vec<&TradePositionWithPhoto> =
        closed.iter().copied().filter(|p| closed_date(p).is_some_and(in_range)).collect();
    let open: Vec<&TradePositionWithPhoto> =
        positions.iter().filter(|p| p.position.status == "OPEN").collect();
    let price_of = |p: &TradePositionWithPhoto| {
        prices.get(&p.position.symbol).copied().unwrap_or(p.position.entry_price)
    };

    let wins: Vec<_> = closed_in_range.iter().filter(|p| p.position.pnl_amount >= Decimal::ZERO).collect();
    let losses: Vec<_> = closed_in_range.iter().filter(|p| p.position.pnl_amount < Decimal::ZERO).collect();
    let realized_pnl: Decimal = closed_in_range.iter().map(|p| p.position.pnl_amount).sum();
    let open_value: Decimal = open.iter().map(|p| p.position.quantity * price_of(p)).sum();
    let open_cost: Decimal = open.iter().map(|p| p.position.entry_value).sum();
    let total_value = cash.map(|c| c + open_value);
    let total_return_amount = match (total_value, initial_capital) {
        (Some(total), Some(initial)) => Some(total - initial),
        _ => None,
    };

    let summary = DashboardSummary {
        initial_capital: initial_capital.map(round_money),
        cash: cash.map(round_money),
        open_value: round_money(open_value),
        total_value: total_value.map(round_money),
        total_return_amount: total_return_amount.map(round_money),
        total_return_percent: match (total_return_amount, initial_capital) {
            (Some(amount), Some(initial)) if !initial.is_zero() => Some(ratio_percent(amount, initial)),
            _ => None,
        },
        realized_pnl: round_money(realized_pnl),
        unrealized_pnl: round_money(open_value - open_cost),
        total_fee: round_money(closed_in_range.iter().chain(open.iter()).map(|p| p.position.fee_amount).sum()),
        closed_trades: closed_in_range.len(),
        open_positions: open.len(),
        win_count: wins.len(),
        loss_count: losses.len(),
        win_rate: ratio_percent(Decimal::from(wins.len()), Decimal::from(closed_in_range.len())),
        avg_profit: round_money(average(wins.iter().map(|p| p.position.pnl_amount).sum(), wins.len())),
        avg_loss: round_money(average(losses.iter().map(|p| p.position.pnl_amount).sum(), losses.len())),
        best_trade: closed_in_range.iter().max_by_key(|p| p.position.pnl_amount).map(|p| highlight(p)),
        worst_trade: closed_in_range.iter().min_by_key(|p| p.position.pnl_amount).map(|p| highlight(p)),
        avg_holding_days: average(
            closed_in_range.iter().map(|p| Decimal::from(p.position.holding_days)).sum(),
            closed_in_range.len(),
        )
        .round_dp(1),
        max_drawdown_percent: max_drawdown,
        profitable_months: monthly.iter().filter(|m| m.pnl > Decimal::ZERO).count(),
        total_months: monthly.len(),
        best_month: monthly.iter().max_by_key(|m| m.pnl).cloned(),
        worst_month: monthly.iter().min_by_key(|m| m.pnl).cloned(),
    };

    // Per aset.
    let mut assets: BTreeMap<String, AssetStats> = BTreeMap::new();
    let mut holding_sum: HashMap<String, Decimal> = HashMap::new();
    let asset_of = |assets: &mut BTreeMap<String, AssetStats>, p: &TradePositionWithPhoto| {
        assets.entry(p.position.symbol.clone()).or_insert_with(|| AssetStats {
            symbol: p.position.symbol.clone(),
            photo_url: p.photo_url.clone(),
            closed_trades: 0,
            wins: 0,
            win_rate: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            contribution_percent: None,
            best_trade: None,
            worst_trade: None,
            avg_holding_days: Decimal::ZERO,
            open_quantity: Decimal::ZERO,
            open_value: Decimal::ZERO,
            unrealized_pnl: Decimal::ZERO,
        });
    };
    for p in &closed_in_range {
        asset_of(&mut assets, p);
        let asset = assets.get_mut(&p.position.symbol).expect("baru dibuat");
        let pnl = p.position.pnl_amount;
        asset.closed_trades += 1;
        if pnl >= Decimal::ZERO {
            asset.wins += 1;
        }
        asset.realized_pnl += pnl;
        asset.best_trade = Some(asset.best_trade.map_or(pnl, |b| b.max(pnl)));
        asset.worst_trade = Some(asset.worst_trade.map_or(pnl, |w| w.min(pnl)));
        *holding_sum.entry(p.position.symbol.clone()).or_default() += Decimal::from(p.position.holding_days);
    }
    for p in &open {
        asset_of(&mut assets, p);
        let asset = assets.get_mut(&p.position.symbol).expect("baru dibuat");
        let value = p.position.quantity * price_of(p);
        asset.open_quantity += p.position.quantity;
        asset.open_value += value;
        asset.unrealized_pnl += value - p.position.entry_value;
    }
    let mut per_asset: Vec<AssetStats> = assets
        .into_values()
        .map(|mut a| {
            a.win_rate = ratio_percent(Decimal::from(a.wins), Decimal::from(a.closed_trades));
            a.contribution_percent = (!realized_pnl.is_zero() && a.closed_trades > 0)
                .then(|| ratio_percent(a.realized_pnl, realized_pnl));
            a.avg_holding_days =
                average(holding_sum.get(&a.symbol).copied().unwrap_or_default(), a.closed_trades).round_dp(1);
            a.realized_pnl = round_money(a.realized_pnl);
            a.best_trade = a.best_trade.map(round_money);
            a.worst_trade = a.worst_trade.map(round_money);
            a.open_value = round_money(a.open_value);
            a.unrealized_pnl = round_money(a.unrealized_pnl);
            a
        })
        .collect();
    per_asset.sort_by_key(|a| std::cmp::Reverse(a.realized_pnl));

    // Isi portofolio sekarang.
    let allocation_total = open_value + cash.unwrap_or_default();
    let mut allocation: Vec<AllocationItem> = per_asset
        .iter()
        .filter(|a| !a.open_value.is_zero())
        .map(|a| AllocationItem {
            symbol: a.symbol.clone(),
            photo_url: a.photo_url.clone(),
            value: a.open_value,
            percent: ratio_percent(a.open_value, allocation_total),
        })
        .collect();
    allocation.sort_by_key(|a| std::cmp::Reverse(a.value));
    if let Some(cash) = cash.filter(|c| !c.is_zero()) {
        allocation.push(AllocationItem {
            symbol: QUOTE_ASSET.to_string(),
            photo_url: None,
            value: round_money(cash),
            percent: ratio_percent(cash, allocation_total),
        });
    }

    Dashboard {
        filter,
        usdt_idr_rate,
        preferred_currency,
        summary,
        equity,
        monthly,
        per_asset,
        allocation,
    }
}

/// Saldo USDT akun Binance user (free + locked). `None` kalau key belum diisi atau gagal dibaca —
/// dashboard tetap tampil tanpa angka saldo.
async fn live_cash(user: &User) -> Option<Decimal> {
    let key = decrypt(user.api_key.as_deref()?).ok()?;
    let secret = decrypt(user.api_secret.as_deref()?).ok()?;
    match binance::get_account_balances(&key, &secret).await {
        Ok(balances) => Some(
            balances
                .into_iter()
                .filter(|b| b.asset == QUOTE_ASSET)
                .map(|b| b.free + b.locked)
                .sum(),
        ),
        Err(err) => {
            tracing::warn!("Dashboard {}: saldo Binance gagal dibaca: {err:?}", user.email);
            None
        }
    }
}

/// Harga sekarang dari memori `TickerHub`; simbol yang belum ada di sana diambil 1x lewat REST.
async fn current_prices(ticker_hub: &TickerHub, symbols: Vec<String>) -> HashMap<String, Decimal> {
    let mut prices: HashMap<String, Decimal> = ticker_hub
        .get(&symbols)
        .into_iter()
        .filter_map(|(symbol, t)| Some((symbol, t.last_price.parse().ok()?)))
        .collect();
    let missing: Vec<String> = symbols.into_iter().filter(|s| !prices.contains_key(s)).collect();
    if !missing.is_empty()
        && let Ok(tickers) = binance::get_tickers_24hr_for(&missing).await {
            prices.extend(tickers.into_iter().filter_map(|t| Some((t.symbol, t.last_price.parse().ok()?))));
        }
    prices
}

/// Dashboard user yang login untuk 1 mode akun (default DEMO), opsional per sumber & rentang tanggal.
/// Data DB dibaca dalam transaksi singkat yang langsung ditutup; harga, kurs & saldo Binance diambil
/// sesudahnya, jadi koneksi DB tidak tertahan selama menunggu exchange.
pub async fn get_dashboard_service(
    pool: &PgPool,
    user_id: Uuid,
    query: DashboardQuery,
    ticker_hub: &TickerHub,
) -> Result<Dashboard, AppError> {
    let account_mode = query.account_mode.as_deref().unwrap_or("DEMO").trim().to_uppercase();
    if !ACCOUNT_MODES.contains(&account_mode.as_str()) {
        return Err(AppError::BadRequest("account_mode harus DEMO atau LIVE".to_string()));
    }
    let source = query.source.as_deref().map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty());
    if source.as_deref().is_some_and(|s| !SOURCES.contains(&s)) {
        return Err(AppError::BadRequest("source harus BOT atau MANUAL".to_string()));
    }
    if let (Some(from), Some(to)) = (query.date_from, query.date_to)
        && from > to {
            return Err(AppError::BadRequest("date_from tidak boleh setelah date_to".to_string()));
        }

    let (user, positions) = {
        let mut tx = begin(pool).await?;
        let user = find_user_by_id(&mut tx, user_id).await?;
        let positions =
            find_trade_positions_for_dashboard(&mut tx, user_id, &account_mode, source.as_deref()).await?;
        commit(tx).await?;
        (user, positions)
    };
    let user = user.ok_or_else(|| AppError::NotFound("User tidak ditemukan".to_string()))?;

    let mut open_symbols: Vec<String> = positions
        .iter()
        .filter(|p| p.position.status == "OPEN")
        .map(|p| p.position.symbol.clone())
        .collect();
    open_symbols.sort();
    open_symbols.dedup();
    let prices = current_prices(ticker_hub, open_symbols).await;

    let (cash, robot_initial) = if account_mode == "DEMO" {
        (Some(user.demo_balance), user.demo_initial_capital)
    } else {
        (live_cash(&user).await, user.platform_initial_capital)
    };
    // Modal awal robot hanya relevan kalau posisi robot ikut dihitung.
    let initial_capital = if source.as_deref() == Some("MANUAL") { None } else { robot_initial };
    let usdt_idr_rate = binance::get_price(USDT_IDR_SYMBOL).await.ok();

    let filter = DashboardFilter {
        account_mode,
        source,
        date_from: query.date_from,
        date_to: query.date_to,
    };
    Ok(build_dashboard(
        filter,
        &positions,
        &prices,
        cash,
        initial_capital,
        usdt_idr_rate,
        user.preferred_currency.clone(),
    ))
}

#[cfg(test)]
mod manual_trade_tests {
    use super::*;

    #[test]
    fn jual_coin_yang_sama_bersamaan_ditolak() {
        let key = "user:DEMO:ADAUSDT".to_string();
        let first = SellingGuard::acquire(key.clone()).expect("pertama boleh");
        assert!(SellingGuard::acquire(key.clone()).is_err(), "kedua ditolak selama yang pertama jalan");
        assert!(SellingGuard::acquire("user:DEMO:BTCUSDT".to_string()).is_ok(), "coin lain tidak terpengaruh");
        drop(first);
        assert!(SellingGuard::acquire(key).is_ok(), "setelah selesai boleh lagi");
    }
}

#[cfg(test)]
mod order_rule_tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// Filter DOGEUSDT dari Binance: minimal 1 DOGE & 1 USDT.
    fn doge() -> binance::TradingRules {
        binance::TradingRules {
            step_size: d("1"),
            min_qty: d("1"),
            max_qty: Some(d("9000000")),
            min_notional: d("1"),
            max_notional: None,
        }
    }

    #[test]
    fn ikut_batas_binance_per_coin() {
        // 2 USDT DOGE lolos (Binance minimal 1 USDT untuk DOGE, bukan 5).
        assert!(check_order(&doge(), "beli", "DOGEUSDT", d("10"), d("2")).is_ok());
        assert!(check_order(&doge(), "beli", "DOGEUSDT", d("4"), d("0.8")).is_err());
        assert!(check_order(&doge(), "jual", "DOGEUSDT", d("0"), d("0")).is_err());
        assert!(check_order(&doge(), "beli", "DOGEUSDT", d("9000001"), d("1800000")).is_err());
    }
}
