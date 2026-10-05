use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, Months, NaiveDate};
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::model::TradePositionWithPhoto;
use super::repository::{
    create_trade_position, find_trade_position_by_id, find_trade_positions_for_dashboard,
    find_trade_positions_paginated,
};
use super::types::{
    AllocationItem, AssetStats, CreateTradePositionInput, Dashboard, DashboardFilter, DashboardQuery,
    DashboardSummary, EquityPoint, MonthlyPnl, NewTradePosition, TradeHighlight,
    TradePositionListQuery,
};
use crate::clients::binance;
use crate::services::market::ticker_hub::TickerHub;
use crate::services::flex_params::repository::find_flex_param_by_id;
use crate::services::users::model::User;
use crate::services::users::repository::find_user_by_id;
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
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    query: &TradePositionListQuery,
    limit: i64,
    offset: i64,
) -> Result<(Vec<TradePositionWithPhoto>, i64), AppError> {
    let search = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let filters = parse_filter_query(query.filter.as_deref())?;
    let sort = parse_sort_query(query.sort.as_deref(), query.order.as_deref())?;

    find_trade_positions_paginated(
        tx,
        user_id,
        search,
        &filters,
        sort.as_ref(),
        &TRADE_POSITION_COLUMNS,
        "opened_at DESC",
        limit,
        offset,
    )
    .await
}

/// Detail 1 posisi milik user yang login. Posisi milik user lain dibalas 404 (sama seperti tidak ada).
pub async fn get_trade_position_by_id_service(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    user_id: Uuid,
) -> Result<TradePositionWithPhoto, AppError> {
    find_trade_position_by_id(tx, id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Posisi trading tidak ditemukan".to_string()))
}

/// Total modal akun LIVE dalam USDT: saldo USDT + nilai semua aset lain. Coin dihitung lewat pair
/// `<ASET>USDT` (qty × harga); mata uang fiat seperti IDR lewat pair kebalikannya `USDT<ASET>`
/// (qty ÷ kurs, mis. Rp1.500.000 ÷ 17.888 di `USDTIDR`). Aset tanpa pair ke USDT sama sekali
/// (mis. token Earn `LD...`) tidak dihitung.
async fn live_account_capital(user: &User) -> Result<Decimal, AppError> {
    let (Some(encrypted_key), Some(encrypted_secret)) = (&user.api_key, &user.api_secret) else {
        return Err(AppError::BadRequest(
            "Akun LIVE butuh API key & secret Binance — isi dulu di profil".to_string(),
        ));
    };
    let api_key = decrypt(encrypted_key)?;
    let api_secret = decrypt(encrypted_secret)?;

    let balances = binance::get_account_balances(&api_key, &api_secret).await?;
    let prices = binance::get_all_prices().await?;

    let mut capital = Decimal::ZERO;
    for balance in balances {
        let amount = balance.free + balance.locked;
        if balance.asset == QUOTE_ASSET {
            capital += amount;
        } else if let Some(price) = prices.get(&format!("{}{QUOTE_ASSET}", balance.asset)) {
            capital += amount * price;
        } else if let Some(rate) = prices
            .get(&format!("{QUOTE_ASSET}{}", balance.asset))
            .filter(|rate| !rate.is_zero())
        {
            capital += amount / rate;
        }
    }
    Ok(capital.round_dp(8))
}

/// Buka posisi baru. Dari FE cukup `flex_param_id`, `entry_value`, `currency`, `account_mode`;
/// sisanya diisi sistem — user & email dari token, simbol dari `flex_params`, harga masuk & kurs
/// USDT/IDR dari Binance, modal akun dari `demo_balance` (DEMO) atau saldo Binance user (LIVE).
/// `entry_value` dalam IDR dikonversi ke USDT pakai kurs saat itu; kurs ikut disimpan supaya nilai
/// posisi bisa dikonversi ke Rupiah kapan pun dengan kurs yang sama.
///
/// Posisi manual hanya dicatat (tidak ada order ke exchange): `source = MANUAL`, tanpa strategi,
/// tanpa orderId, bukti order & fee, `quantity` = `entry_value / entry_price`.
///
/// Urutan: baca user & coin (transaksi singkat) -> ambil kurs, harga & saldo dari Binance (tanpa
/// transaksi) -> simpan posisi (transaksi singkat). Koneksi DB tidak tertahan selama menunggu exchange.
pub async fn create_trade_position_service(
    pool: &PgPool,
    user_id: Uuid,
    user_email: &str,
    input: CreateTradePositionInput,
) -> Result<TradePositionWithPhoto, AppError> {
    let account_mode = input.account_mode.trim().to_uppercase();
    if !ACCOUNT_MODES.contains(&account_mode.as_str()) {
        return Err(AppError::BadRequest("account_mode harus DEMO atau LIVE".to_string()));
    }
    if input.entry_value <= Decimal::ZERO {
        return Err(AppError::BadRequest("entry_value harus lebih dari 0".to_string()));
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

    let usdt_idr_rate = binance::get_price(USDT_IDR_SYMBOL).await.map_err(|_| {
        AppError::BadRequest("Gagal mengambil kurs USDT/IDR dari Binance".to_string())
    })?;
    let entry_value = match currency.as_str() {
        "IDR" => (input.entry_value / usdt_idr_rate).round_dp(8),
        _ => input.entry_value,
    };

    let account_capital = match account_mode.as_str() {
        "DEMO" => user.demo_balance,
        _ => live_account_capital(&user).await?,
    };
    if entry_value > account_capital {
        return Err(AppError::BadRequest(format!(
            "entry_value {entry_value} USDT melebihi modal akun {account_mode} ({account_capital} USDT)"
        )));
    }

    let entry_price = binance::get_price(&coin.value_param).await.map_err(|_| {
        AppError::BadRequest(format!("Gagal mengambil harga {} dari Binance", coin.value_param))
    })?;

    let mut tx = begin(pool).await?;
    let position = create_trade_position(
        &mut tx,
        &NewTradePosition {
            user_id,
            user_email: user_email.to_string(),
            flex_param_id: coin.id,
            symbol: coin.value_param.clone(),
            source: "MANUAL".to_string(),
            strategy: None,
            opened_at: chrono::Utc::now(),
            closed_at: None,
            holding_days: 0,
            currency,
            usdt_idr_rate,
            quantity: (entry_value / entry_price).round_dp(8),
            entry_value,
            exit_value: None,
            entry_price,
            exit_price: None,
            fee_amount: Decimal::ZERO,
            entry_order_id: None,
            exit_order_id: None,
            entry_order_response: None,
            exit_order_response: None,
            pnl_amount: Decimal::ZERO,
            pnl_percent: Decimal::ZERO,
            status: "OPEN".to_string(),
            result: None,
            account_capital,
            account_mode,
            created_by: user_email.to_string(),
        },
    )
    .await?;
    commit(tx).await?;

    Ok(TradePositionWithPhoto {
        position,
        photo_url: coin.photo_url,
    })
}

async fn begin(pool: &PgPool) -> Result<Transaction<'_, Postgres>, AppError> {
    pool.begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))
}

async fn commit(tx: Transaction<'_, Postgres>) -> Result<(), AppError> {
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))
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
    per_asset.sort_by(|a, b| b.realized_pnl.cmp(&a.realized_pnl));

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
    allocation.sort_by(|a, b| b.value.cmp(&a.value));
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
    if !missing.is_empty() {
        if let Ok(tickers) = binance::get_tickers_24hr_for(&missing).await {
            prices.extend(tickers.into_iter().filter_map(|t| Some((t.symbol, t.last_price.parse().ok()?))));
        }
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
    if let (Some(from), Some(to)) = (query.date_from, query.date_to) {
        if from > to {
            return Err(AppError::BadRequest("date_from tidak boleh setelah date_to".to_string()));
        }
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
