use std::str::FromStr;
use std::time::Duration;

use futures_util::StreamExt;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

use super::repository::insert_candle;
use crate::services::flex_params::repository::find_flex_params_by_type;
use crate::services::market::types::RawKlineEvent;

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
/// Strategi swing trading kamu pakai timeframe 1D — collector fokus kumpulkan candle harian.
const COLLECTOR_INTERVAL: &str = "1d";
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

fn ws_base_url() -> String {
    std::env::var("MARKET_WS_BASE_URL")
        .unwrap_or_else(|_| "wss://stream-cloud.tokocrypto.site/ws".to_string())
}

/// Jalan sebagai background task selama server hidup. Ambil daftar simbol dari `flex_params`
/// (type_param=SIMBOL_CRYPTO), lalu buka 1 koneksi WebSocket per simbol ke Tokocrypto —
/// setiap candle yang CLOSED langsung disimpan ke `market_candles`. Auto-reconnect kalau putus,
/// tidak bergantung sama sekali pada ada/tidaknya client yang buka endpoint /ws milik FE.
pub async fn start(pool: PgPool) {
    let symbols = match load_symbols(&pool).await {
        Ok(symbols) if !symbols.is_empty() => symbols,
        Ok(_) => {
            tracing::warn!(
                "Tidak ada simbol di flex_params (type_param={SYMBOL_TYPE_PARAM}), collector tidak dijalankan"
            );
            return;
        }
        Err(err) => {
            tracing::error!("Gagal mengambil daftar simbol untuk market data collector: {err:?}");
            return;
        }
    };

    tracing::info!(
        "Market data collector dimulai untuk {} simbol, interval {COLLECTOR_INTERVAL}: {:?}",
        symbols.len(),
        symbols
    );

    for symbol in symbols {
        let pool = pool.clone();
        tokio::spawn(collect_symbol(pool, symbol));
    }
}

async fn load_symbols(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let flex_params = find_flex_params_by_type(&mut tx, SYMBOL_TYPE_PARAM, true)
        .await
        .map_err(|_| sqlx::Error::RowNotFound)?;
    tx.commit().await?;

    Ok(flex_params
        .into_iter()
        .map(|fp| fp.value_param)
        .collect())
}

/// Loop tak terbatas untuk 1 simbol: connect, dengarkan candle closed, simpan ke DB.
/// Kalau koneksi putus (network error, server restart Tokocrypto, dll), tunggu sebentar lalu
/// coba connect lagi — supaya data historis tidak bolong walau ada gangguan sementara.
async fn collect_symbol(pool: PgPool, symbol: String) {
    let stream_symbol = symbol.to_lowercase();

    loop {
        let base_url = ws_base_url();
        let stream_url = format!("{base_url}/{stream_symbol}@kline_{COLLECTOR_INTERVAL}");

        match tokio_tungstenite::connect_async(&stream_url).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Market data collector terhubung: {symbol} ({COLLECTOR_INTERVAL})");
                let (_, mut read) = ws_stream.split();

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(UpstreamMessage::Text(text)) => {
                            handle_message(&pool, &symbol, &text).await;
                        }
                        Ok(UpstreamMessage::Close(_)) | Err(_) => break,
                        _ => {}
                    }
                }

                tracing::warn!("Market data collector terputus dari {symbol}, mencoba menyambung ulang");
            }
            Err(err) => {
                tracing::error!("Market data collector gagal connect ke {symbol}: {err}");
            }
        }

        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn handle_message(pool: &PgPool, symbol: &str, text: &str) {
    let Ok(event) = serde_json::from_str::<RawKlineEvent>(text) else {
        return;
    };

    // hanya simpan candle yang sudah final — candle yang masih berjalan sengaja diabaikan
    // supaya dataset ML tidak tercemar data sementara yang masih bisa berubah.
    if !event.k.is_closed {
        return;
    }

    let (Some(open), Some(high), Some(low), Some(close), Some(volume)) = (
        Decimal::from_str(&event.k.open).ok(),
        Decimal::from_str(&event.k.high).ok(),
        Decimal::from_str(&event.k.low).ok(),
        Decimal::from_str(&event.k.close).ok(),
        Decimal::from_str(&event.k.volume).ok(),
    ) else {
        tracing::error!("Gagal parsing angka candle untuk {symbol}, event dilewati");
        return;
    };

    if let Err(err) = insert_candle(
        pool,
        symbol,
        COLLECTOR_INTERVAL,
        event.k.open_time,
        event.k.close_time,
        open,
        high,
        low,
        close,
        volume,
    )
    .await
    {
        tracing::error!("Gagal menyimpan candle {symbol} ({COLLECTOR_INTERVAL}): {err:?}");
    }
}
