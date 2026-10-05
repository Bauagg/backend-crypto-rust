//! Hub harga live semua coin: 1 koneksi WebSocket ke exchange untuk SEMUA simbol aktif
//! (`<symbol>@miniTicker`), dipakai bersama semua client. 1 user atau 10.000 user yang membuka
//! daftar coin = tetap 1 koneksi ke exchange, dan tidak memakai kuota REST sama sekali.
//!
//! Harga terbaru disimpan di memori — `GET /symbols` membacanya tanpa request ke exchange — dan
//! perubahan dikumpulkan lalu disiarkan tiap `BROADCAST_INTERVAL` ke client `/ws/tickers`.
//! Hanya simbol aktif di `flex_params` yang di-subscribe (bukan `!miniTicker@arr` yang berisi
//! ±3.500 pair), supaya data masuk ke server tetap kecil.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures_util::{Sink, SinkExt, StreamExt};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

use super::types::MarketTicker;
use crate::clients::binance;
use crate::services::flex_params::repository::find_flex_params_by_type;
use crate::utils::app_error::AppError;

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
/// Buffer batch per client. Client yang tertinggal cuma melewatkan batch lama.
const CHANNEL_CAPACITY: usize = 16;
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
/// Perubahan harga dikumpulkan lalu dikirim sekaligus — 1 pesan/detik ke client, bukan 1 per coin.
const BROADCAST_INTERVAL: Duration = Duration::from_secs(1);
/// Daftar simbol dibaca ulang dari `flex_params` — coin baru/nonaktif hasil sync ikut tanpa restart.
const SYMBOL_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Batas exchange: maksimal 1024 stream per koneksi.
const MAX_STREAMS: usize = 1024;
/// Stream per pesan SUBSCRIBE, dengan jeda antar pesan — batas exchange 5 pesan masuk/detik.
const COMMAND_CHUNK: usize = 200;
const COMMAND_GAP: Duration = Duration::from_millis(300);

pub struct TickerHub {
    prices: RwLock<HashMap<String, MarketTicker>>,
    updates: broadcast::Sender<Arc<Vec<MarketTicker>>>,
}

impl TickerHub {
    pub fn new() -> Arc<Self> {
        let (updates, _) = broadcast::channel(CHANNEL_CAPACITY);
        Arc::new(Self {
            prices: RwLock::new(HashMap::new()),
            updates,
        })
    }

    /// Harga terbaru simbol-simbol ini dari memori. Simbol yang belum pernah menerima update
    /// (mis. server baru start) tidak ada di hasil — pemanggil yang menentukan fallback-nya.
    pub fn get(&self, symbols: &[String]) -> HashMap<String, MarketTicker> {
        let prices = self.prices.read().expect("lock harga tidak boleh poisoned");
        symbols
            .iter()
            .filter_map(|symbol| Some((symbol.clone(), prices.get(symbol)?.clone())))
            .collect()
    }

    /// Batch perubahan harga tiap `BROADCAST_INTERVAL` (semua simbol; client memfilter sendiri).
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Vec<MarketTicker>>> {
        self.updates.subscribe()
    }

    /// Jalan sebagai background task selama server hidup. Auto-reconnect kalau putus (exchange
    /// memutus koneksi tiap 24 jam, gangguan jaringan, dll).
    pub async fn run(self: Arc<Self>, pool: PgPool) {
        loop {
            match load_symbols(&pool).await {
                Ok(symbols) if symbols.is_empty() => {
                    tracing::warn!("Stream ticker: belum ada simbol aktif di flex_params");
                    tokio::time::sleep(SYMBOL_REFRESH_INTERVAL).await;
                    continue;
                }
                Ok(symbols) => match tokio_tungstenite::connect_async(binance::stream_base_url()).await {
                    Ok((upstream, _)) => {
                        tracing::info!("Stream ticker dibuka: {} simbol", symbols.len());
                        self.stream(upstream, symbols, &pool).await;
                        tracing::warn!("Stream ticker terputus, menyambung ulang");
                    }
                    Err(err) => tracing::error!("Gagal connect stream ticker: {err}"),
                },
                Err(err) => tracing::error!("Stream ticker gagal membaca simbol: {err:?}"),
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }

    /// Satu sesi koneksi: subscribe, terima harga, siarkan batch, dan sesuaikan subscribe kalau
    /// daftar simbol berubah. Kembali (lalu reconnect) kalau koneksi putus.
    async fn stream<S>(&self, upstream: S, mut symbols: HashSet<String>, pool: &PgPool)
    where
        S: futures_util::Stream<Item = Result<UpstreamMessage, tokio_tungstenite::tungstenite::Error>>
            + Sink<UpstreamMessage>
            + Unpin,
    {
        let (mut write, mut read) = upstream.split();
        let initial: Vec<String> = symbols.iter().cloned().collect();
        if send_command(&mut write, "SUBSCRIBE", &initial).await.is_err() {
            return;
        }

        let mut flush = tokio::time::interval(BROADCAST_INTERVAL);
        let mut refresh = tokio::time::interval(SYMBOL_REFRESH_INTERVAL);
        refresh.tick().await; // tick pertama langsung selesai; baru dicek setelah 1 interval
        let mut changed: HashMap<String, MarketTicker> = HashMap::new();

        loop {
            tokio::select! {
                msg = read.next() => {
                    let text = match msg {
                        Some(Ok(UpstreamMessage::Text(text))) => text,
                        Some(Ok(UpstreamMessage::Close(_))) | Some(Err(_)) | None => return,
                        _ => continue,
                    };
                    // Balasan SUBSCRIBE (`{"result":null,"id":..}`) tidak punya field ticker -> dilewati.
                    let Ok(event) = serde_json::from_str::<binance::MiniTickerEvent>(&text) else {
                        continue;
                    };
                    let Some(ticker) = MarketTicker::from_mini_ticker(event) else { continue };
                    self.prices
                        .write()
                        .expect("lock harga tidak boleh poisoned")
                        .insert(ticker.symbol.clone(), ticker.clone());
                    changed.insert(ticker.symbol.clone(), ticker);
                }
                _ = flush.tick() => {
                    if !changed.is_empty() {
                        let batch: Vec<MarketTicker> = changed.drain().map(|(_, t)| t).collect();
                        // Gagal kirim = sedang tidak ada client; harga tetap tersimpan di memori.
                        let _ = self.updates.send(Arc::new(batch));
                    }
                }
                _ = refresh.tick() => {
                    let latest = match load_symbols(pool).await {
                        Ok(latest) if !latest.is_empty() => latest,
                        _ => continue,
                    };
                    let added: Vec<String> = latest.difference(&symbols).cloned().collect();
                    let removed: Vec<String> = symbols.difference(&latest).cloned().collect();
                    if added.is_empty() && removed.is_empty() {
                        continue;
                    }
                    if send_command(&mut write, "UNSUBSCRIBE", &removed).await.is_err()
                        || send_command(&mut write, "SUBSCRIBE", &added).await.is_err()
                    {
                        return;
                    }
                    {
                        let mut prices = self.prices.write().expect("lock harga tidak boleh poisoned");
                        for symbol in &removed {
                            prices.remove(symbol);
                        }
                    }
                    tracing::info!(
                        "Stream ticker: +{} simbol, -{} simbol (total {})",
                        added.len(),
                        removed.len(),
                        latest.len()
                    );
                    symbols = latest;
                }
            }
        }
    }
}

/// Simbol aktif (`SIMBOL_CRYPTO`, tidak dihapus, `is_active`) — maksimal `MAX_STREAMS`.
async fn load_symbols(pool: &PgPool) -> Result<HashSet<String>, AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))?;
    let params = find_flex_params_by_type(&mut tx, SYMBOL_TYPE_PARAM, true).await?;
    let _ = tx.commit().await;

    Ok(params
        .into_iter()
        .map(|fp| fp.value_param.to_uppercase())
        .take(MAX_STREAMS)
        .collect())
}

/// Kirim SUBSCRIBE/UNSUBSCRIBE dipecah per `COMMAND_CHUNK` stream, berjeda supaya tidak melewati
/// batas pesan masuk exchange.
async fn send_command<W>(write: &mut W, method: &str, symbols: &[String]) -> Result<(), ()>
where
    W: Sink<UpstreamMessage> + Unpin,
{
    for (i, chunk) in symbols.chunks(COMMAND_CHUNK).enumerate() {
        if i > 0 {
            tokio::time::sleep(COMMAND_GAP).await;
        }
        let streams: Vec<String> = chunk.iter().map(|s| binance::mini_ticker_stream(s)).collect();
        let command = binance::stream_command(method, &streams, i as u64 + 1);
        write.send(UpstreamMessage::Text(command)).await.map_err(|_| ())?;
    }
    Ok(())
}
