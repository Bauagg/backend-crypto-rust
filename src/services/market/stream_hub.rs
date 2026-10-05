//! Hub stream candle live: 1 koneksi WebSocket ke exchange per simbol+interval, dipakai bersama
//! (fan-out) oleh semua client yang membuka chart yang sama. 50 user membuka BTCUSDT 1h = tetap
//! 1 koneksi ke exchange, jadi jumlah koneksi ke exchange tidak ikut naik dengan jumlah user.
//!
//! Tiap event candle sekaligus ditulis ke Redis (`repository`): candle yang masih berjalan ke key
//! live, candle yang baru close ke cache rentang — histori di Redis selalu sambung sampai candle
//! terbaru selama ada yang membuka chart itu (seperti bar terakhir di MT5).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

use super::repository;
use super::types::Candle;
use crate::clients::binance;
use crate::database::RedisPool;

/// Buffer pesan per stream. Client yang tertinggal lebih dari ini cuma melewatkan update lama
/// (candle live berikutnya tetap menyusul), tidak memperlambat client lain.
const CHANNEL_CAPACITY: usize = 64;
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

type StreamKey = (String, String);

pub struct KlineHub {
    redis: RedisPool,
    streams: Mutex<HashMap<StreamKey, broadcast::Sender<Arc<String>>>>,
}

impl KlineHub {
    pub fn new(redis: RedisPool) -> Arc<Self> {
        Arc::new(Self {
            redis,
            streams: Mutex::new(HashMap::new()),
        })
    }

    /// Daftar ke stream simbol+interval. Kalau belum ada yang membukanya, koneksi ke exchange
    /// dibuat saat itu juga; kalau sudah ada, client langsung ikut menerima stream yang sama.
    /// Pesan berisi JSON `Candle` (bentuk sama dengan REST `/klines`).
    pub fn subscribe(self: &Arc<Self>, symbol: &str, interval: &str) -> broadcast::Receiver<Arc<String>> {
        let key = (symbol.to_uppercase(), interval.to_string());
        let mut streams = self.streams.lock().expect("mutex hub tidak boleh poisoned");

        if let Some(sender) = streams.get(&key) {
            return sender.subscribe();
        }

        let (sender, receiver) = broadcast::channel(CHANNEL_CAPACITY);
        streams.insert(key.clone(), sender.clone());
        tokio::spawn(Arc::clone(self).run_upstream(key, sender));
        receiver
    }

    /// Hapus stream kalau sudah tidak ada client. Dicek di bawah lock yang sama dengan `subscribe`,
    /// jadi client yang baru masuk tepat saat stream mau ditutup tidak tertinggal di stream mati.
    fn remove_if_unused(&self, key: &StreamKey, sender: &broadcast::Sender<Arc<String>>) -> bool {
        let mut streams = self.streams.lock().expect("mutex hub tidak boleh poisoned");
        if sender.receiver_count() > 0 {
            return false;
        }
        streams.remove(key);
        tracing::info!("Stream kline {}@{} ditutup (tidak ada client)", key.0, key.1);
        true
    }

    /// Satu koneksi ke exchange untuk 1 simbol+interval, hidup selama masih ada client.
    /// Auto-reconnect kalau putus (exchange memutus koneksi tiap 24 jam, gangguan jaringan, dll).
    async fn run_upstream(self: Arc<Self>, key: StreamKey, sender: broadcast::Sender<Arc<String>>) {
        let (symbol, interval) = (&key.0, &key.1);
        let url = binance::kline_stream_url(symbol, interval);
        tracing::info!("Stream kline {symbol}@{interval} dibuka");

        loop {
            match tokio_tungstenite::connect_async(&url).await {
                Ok((upstream, _)) => {
                    let (_, mut read) = upstream.split();
                    while let Some(msg) = read.next().await {
                        let text = match msg {
                            Ok(UpstreamMessage::Text(text)) => text,
                            Ok(UpstreamMessage::Close(_)) | Err(_) => break,
                            _ => continue,
                        };
                        let Ok(event) = serde_json::from_str::<binance::KlineEvent>(&text) else {
                            continue;
                        };
                        let candle = Candle::from(event);
                        // Serialize sekali, dipakai untuk cache Redis & dikirim ke client.
                        let Ok(payload) = serde_json::to_string(&candle) else { continue };
                        self.write_to_cache(symbol, interval, &candle, &payload).await;

                        // Gagal kirim = sedang tidak ada client; dicek di bawah.
                        let _ = sender.send(Arc::new(payload));
                        if sender.receiver_count() == 0 && self.remove_if_unused(&key, &sender) {
                            return;
                        }
                    }
                    tracing::warn!("Stream kline {symbol}@{interval} terputus, menyambung ulang");
                }
                Err(err) => tracing::error!("Gagal connect stream kline {symbol}@{interval}: {err}"),
            }

            if self.remove_if_unused(&key, &sender) {
                return;
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }

    async fn write_to_cache(&self, symbol: &str, interval: &str, candle: &Candle, payload: &str) {
        if candle.is_closed {
            // Hanya 1x per candle (saat close) — tetap lewat jalur histori yang sama dengan REST.
            repository::store_closed(&self.redis, symbol, interval, std::slice::from_ref(candle), false)
                .await;
        } else {
            repository::store_live(&self.redis, symbol, interval, payload).await;
        }
    }
}
