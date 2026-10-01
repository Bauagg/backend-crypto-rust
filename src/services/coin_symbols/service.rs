use std::collections::{HashMap, HashSet};

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::types::{CoinGeckoCoin, SyncSummary};
use crate::clients::{binance, coingecko};
use crate::services::flex_params::repository::{
    find_deleted_flex_params_by_type, find_flex_params_by_type, restore_flex_param,
    soft_delete_flex_param,
};
use crate::services::flex_params::service::{
    create_flex_param_service, update_flex_param_service, PhotoUpload,
};
use crate::services::flex_params::types::{CreateFlexParamInput, UpdateFlexParamInput};
use crate::utils::app_error::AppError;
use crate::utils::http_client::get_bytes;

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
const QUOTE_ASSET: &str = "USDT";
const SYSTEM_ACTOR: &str = "system";
/// `description` SIMBOL_CRYPTO = kategori market cap (`Large`/`Mid`/`Small`), dibaca API Python
/// (backtes-crypto) untuk menentukan universe strategi. Dihitung dari market cap CoinGecko.
const LARGE_CAP_MIN_USD: f64 = 50_000_000_000.0;
const MID_CAP_MIN_USD: f64 = 5_000_000_000.0;
const DEFAULT_MARKET_CAP_CATEGORY: &str = "Small";
/// Basket strategi `/signal` V23 di bot Python — tidak pernah di-soft-delete sync (bot butuh
/// candle ke-8 coin ini lengkap) dan kategorinya dikunci, supaya hasil riset/backtest bot tidak
/// bergeser karena volume atau market cap naik turun.
const LOCKED_SYMBOLS: [&str; 8] = [
    "BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "LINKUSDT", "TRXUSDT", "DOGEUSDT",
];
/// Coin baru masuk / coin yang dihapus sistem dipulihkan kalau volume 24 jam >= ini (USDT).
const MIN_VOLUME_TO_ENTER: f64 = 5_000_000.0;
/// Coin yang sudah ada baru di-soft-delete kalau volume turun di bawah ini — sengaja lebih rendah
/// dari ambang masuk supaya coin di sekitar batas tidak keluar-masuk daftar tiap hari (universe
/// bot Python jadi stabil).
const MIN_VOLUME_TO_STAY: f64 = 3_000_000.0;
/// Harganya diam di ~1 (dipatok ke USD/EUR) — tidak berguna untuk trading cari profit
/// walau volumenya besar.
const STABLECOINS: [&str; 16] = [
    "USDC", "FDUSD", "TUSD", "USDP", "BUSD", "DAI", "PYUSD", "USDE", "USDS", "USD1", "RLUSD",
    "BFUSD", "XUSD", "AEUR", "EURI", "EUR",
];
/// Token yang dipatok ke harga emas — aset komoditas, bukan crypto.
const GOLD_TOKENS: [&str; 2] = ["PAXG", "XAUT"];
/// CoinGecko ikut mendaftarkan saham tokenized (mis. "Circle Internet Group (bStocks Tokenized
/// Stock)") — dikenali dari namanya, dibuang supaya daftar coin murni crypto.
const TOKENIZED_STOCK_MARKER: &str = "tokenized stock";
/// Logo coin cukup kecil (~10-50 KB); batas ini mencegah URL nyasar ikut diunduh.
const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;
/// Jumlah coin teratas CoinGecko (berdasarkan market cap) yang dianggap "crypto besar".
const COINGECKO_TOP_N: u16 = 250;

/// Top 250 crypto (market cap + URL logo) dari CoinGecko, dipetakan per ticker (huruf besar).
/// Saham tokenized dibuang. Respons terurut market cap terbesar dulu, jadi kalau ada ticker kembar
/// yang dipakai yang terbesar. Daftar ini sekaligus jadi penyaring "crypto asli": pair Binance yang
/// base asset-nya tidak ada di sini (saham tokenized, coin kecil yang sedang di-pump) tidak masuk.
async fn fetch_coingecko_coins() -> Result<HashMap<String, CoinGeckoCoin>, AppError> {
    let coins = coingecko::get_top_markets(COINGECKO_TOP_N).await?;

    let mut result = HashMap::new();
    for coin in coins {
        if coin.name.to_lowercase().contains(TOKENIZED_STOCK_MARKER) {
            continue;
        }
        if let Some(market_cap) = coin.market_cap {
            result
                .entry(coin.symbol.to_uppercase())
                .or_insert(CoinGeckoCoin {
                    market_cap,
                    image: coin.image,
                });
        }
    }
    Ok(result)
}

fn market_cap_category(market_cap: f64) -> &'static str {
    match market_cap {
        cap if cap >= LARGE_CAP_MIN_USD => "Large",
        cap if cap >= MID_CAP_MIN_USD => "Mid",
        _ => DEFAULT_MARKET_CAP_CATEGORY,
    }
}

/// UUID user pemilik baris yang dibuat otomatis oleh sync (`flex_params.user_id` wajib diisi).
fn system_user_id() -> Option<Uuid> {
    std::env::var("SYSTEM_USER_ID").ok().and_then(|v| Uuid::parse_str(v.trim()).ok())
}

/// Pair USDT spot yang sedang TRADING di exchange, dipetakan ke base asset-nya. Simbol non-ASCII
/// (mis. nama coin berhuruf China) dilewati supaya aman dipakai di URL/query.
async fn fetch_trading_pairs() -> Result<HashMap<String, String>, AppError> {
    Ok(binance::get_spot_symbols()
        .await?
        .into_iter()
        .filter(|s| {
            s.quote_asset == QUOTE_ASSET
                && s.status == "TRADING"
                && s.is_spot_trading_allowed
                && s.symbol.is_ascii()
        })
        .map(|s| (s.symbol, s.base_asset))
        .collect())
}

/// Volume 24 jam (USDT) semua simbol dalam 1 request.
async fn fetch_volumes() -> Result<HashMap<String, f64>, AppError> {
    Ok(binance::get_tickers_24hr()
        .await?
        .into_iter()
        .filter_map(|t| Some((t.symbol, t.quote_volume.parse().ok()?)))
        .collect())
}

fn image_extension(mime_type: &str) -> Option<&'static str> {
    match mime_type.split(';').next()?.trim() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

/// Unduh logo coin dari URL CoinGecko. `None` (cukup di-log) kalau gagal — coin tetap tersimpan
/// non-aktif dan logonya dicoba lagi di sync berikutnya.
async fn download_logo(symbol: &str, image_url: Option<&str>) -> Option<PhotoUpload> {
    let image_url = image_url?;
    let (bytes, content_type) = match get_bytes(image_url, MAX_LOGO_BYTES).await {
        Ok(result) => result,
        Err(err) => {
            tracing::warn!("Gagal unduh logo {symbol} dari {image_url}: {err:?}");
            return None;
        }
    };
    let Some(extension) = image_extension(&content_type) else {
        tracing::warn!("Logo {symbol} bukan gambar ({content_type}): {image_url}");
        return None;
    };

    Some(PhotoUpload {
        original_name: format!("{symbol}.{extension}"),
        mime_type: content_type,
        bytes,
    })
}

/// Samakan daftar `SIMBOL_CRYPTO` di `flex_params` dengan pair USDT spot di exchange.
/// Coin "besar" = crypto asli (ada di top 250 CoinGecko, bukan saham tokenized), sedang trading,
/// bukan stablecoin/token emas, dengan volume 24 jam di atas ambang (histeresis):
/// - sudah ada, volume < `MIN_VOLUME_TO_STAY` (atau bukan crypto asli lagi) -> soft delete oleh
///   `system` (foto tidak dihapus);
/// - pernah di-soft-delete oleh `system`, volume >= `MIN_VOLUME_TO_ENTER` -> dipulihkan (foto lama kembali);
/// - belum pernah ada, volume >= `MIN_VOLUME_TO_ENTER` -> dibuat lewat `create_flex_param_service`.
///
/// `LOCKED_SYMBOLS` (basket V23 bot Python) tidak pernah di-soft-delete sync, apa pun volumenya.
///
/// Coin yang belum punya foto langsung diberi logo dari CoinGecko. `is_active` mengikuti foto:
/// aktif hanya kalau punya foto (kalau unduhan logo gagal, coin non-aktif dulu dan dicoba lagi
/// sync berikutnya). Baris yang di-soft-delete user (mis. coin scam) tidak pernah
/// dipulihkan/dibuat ulang — user tetap pegang kendali penuh untuk membuang coin tertentu.
///
/// `description` (kategori market cap) dihitung ulang dari CoinGecko, kecuali basket
/// `LOCKED_SYMBOLS`. CoinGecko wajib berhasil — tanpanya saham & crypto tidak bisa
/// dibedakan, jadi sync dibatalkan (dicoba lagi di putaran berikutnya).
pub async fn sync_coin_symbols_service(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<SyncSummary, AppError> {
    let trading_pairs = fetch_trading_pairs().await?;
    let volumes = fetch_volumes().await?;
    let coingecko = fetch_coingecko_coins().await?;

    // Pengaman: respons kosong (API error/maintenance) jangan sampai menghapus semua coin.
    if trading_pairs.is_empty() || volumes.is_empty() || coingecko.is_empty() {
        return Err(AppError::Internal(
            "Data pair/volume/market cap kosong, sync dibatalkan".to_string(),
        ));
    }

    let coin_of = |symbol: &str| trading_pairs.get(symbol).and_then(|base| coingecko.get(base));
    let category_of = |symbol: &str| {
        coin_of(symbol)
            .map(|coin| market_cap_category(coin.market_cap))
            .unwrap_or(DEFAULT_MARKET_CAP_CATEGORY)
    };
    let logo_url_of = |symbol: &str| coin_of(symbol).and_then(|coin| coin.image.as_deref());

    let is_real_crypto = |symbol: &str| {
        trading_pairs.get(symbol).is_some_and(|base| {
            coingecko.contains_key(base)
                && !STABLECOINS.contains(&base.as_str())
                && !GOLD_TOKENS.contains(&base.as_str())
        })
    };
    let volume_of = |symbol: &str| volumes.get(symbol).copied().unwrap_or(0.0);
    // Boleh masuk/dipulihkan.
    let can_enter = |symbol: &str| is_real_crypto(symbol) && volume_of(symbol) >= MIN_VOLUME_TO_ENTER;
    // Boleh tetap di daftar (coin yang sudah ada).
    let can_stay = |symbol: &str| {
        LOCKED_SYMBOLS.contains(&symbol)
            || (is_real_crypto(symbol) && volume_of(symbol) >= MIN_VOLUME_TO_STAY)
    };

    let mut summary = SyncSummary::default();

    // 1. Pulihkan yang dulu dihapus sistem tapi kini coin besar lagi. Hasil query terurut
    //    terbaru dulu per simbol — cukup lihat penghapusan terakhirnya.
    let mut known: HashSet<String> = find_flex_params_by_type(tx, SYMBOL_TYPE_PARAM, false)
        .await?
        .into_iter()
        .map(|p| p.value_param)
        .collect();

    for (id, symbol, deleted_by, has_photo) in
        find_deleted_flex_params_by_type(tx, SYMBOL_TYPE_PARAM).await?
    {
        if !known.insert(symbol.clone()) {
            continue;
        }
        let should_restore = LOCKED_SYMBOLS.contains(&symbol.as_str()) || can_enter(&symbol);
        if deleted_by.as_deref() == Some(SYSTEM_ACTOR) && should_restore {
            restore_flex_param(tx, id, has_photo, SYSTEM_ACTOR).await?;
            summary.restored += 1;
        }
    }

    // 2. Semua baris hidup (termasuk yang baru dipulihkan): hapus yang tidak layak tetap, lengkapi
    //    foto yang belum ada, samakan is_active dengan foto, perbarui kategori market cap.
    for param in find_flex_params_by_type(tx, SYMBOL_TYPE_PARAM, false).await? {
        let symbol = param.value_param.as_str();
        if !can_stay(symbol) {
            soft_delete_flex_param(tx, param.id, SYSTEM_ACTOR).await?;
            summary.deleted += 1;
            continue;
        }

        let photo = match param.photo_url {
            Some(_) => None,
            None => download_logo(symbol, logo_url_of(symbol)).await,
        };
        let has_photo = param.photo_url.is_some() || photo.is_some();
        let new_category = Some(category_of(symbol))
            .filter(|_| !LOCKED_SYMBOLS.contains(&symbol))
            .filter(|category| param.description.as_deref() != Some(*category));

        if param.is_active == has_photo && new_category.is_none() && photo.is_none() {
            continue;
        }

        if photo.is_some() {
            summary.photos += 1;
        }
        if new_category.is_some() {
            summary.recategorized += 1;
        }

        let input = UpdateFlexParamInput {
            is_active: Some(has_photo),
            description: new_category.map(str::to_string),
            ..Default::default()
        };
        update_flex_param_service(tx, param.id, input, SYSTEM_ACTOR, photo).await?;
    }

    // 3. Coin besar yang belum pernah ada sama sekali — langsung dengan logo CoinGecko.
    let mut new_symbols: Vec<&String> = trading_pairs
        .keys()
        .filter(|symbol| !known.contains(*symbol) && can_enter(symbol))
        .collect();
    new_symbols.sort();

    if !new_symbols.is_empty() {
        let Some(user_id) = system_user_id() else {
            tracing::warn!(
                "SYSTEM_USER_ID belum di-set/tidak valid di .env, {} coin baru dilewati",
                new_symbols.len()
            );
            return Ok(summary);
        };

        for symbol in new_symbols {
            let photo = download_logo(symbol, logo_url_of(symbol)).await;
            if photo.is_some() {
                summary.photos += 1;
            }

            let input = CreateFlexParamInput {
                type_param: SYMBOL_TYPE_PARAM.to_string(),
                value_param: symbol.clone(),
                description: Some(category_of(symbol).to_string()),
                header_id: None,
                is_active: photo.is_some(),
            };
            create_flex_param_service(tx, input, user_id, SYSTEM_ACTOR, photo).await?;
            summary.created += 1;
        }
    }

    Ok(summary)
}
