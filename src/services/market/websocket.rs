use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use super::stream_hub::KlineHub;

/// Kirim candle live (update tiap ±1-2 detik, candle bergerak real-time) ke client kita. Data
/// diambil dari `KlineHub` — 1 koneksi ke exchange per simbol+interval dipakai bersama semua
/// client, bukan 1 koneksi per client. Bentuk pesan = JSON `Candle`, sama persis dengan REST
/// /klines, supaya frontend cuma perlu satu struktur data untuk histori & live.
pub async fn relay_kline_stream(
    client_socket: WebSocket,
    hub: Arc<KlineHub>,
    symbol: String,
    interval: String,
) {
    let mut updates = hub.subscribe(&symbol, &interval);
    let (mut client_write, mut client_read) = client_socket.split();

    let forward_to_client = async {
        loop {
            match updates.recv().await {
                Ok(payload) => {
                    if client_write.send(Message::Text(payload.to_string())).await.is_err() {
                        break;
                    }
                }
                // Client terlalu lambat & melewatkan beberapa update lama — lanjut dari yang terbaru.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };

    // Client disconnect / kirim close -> berhenti. Stream ke exchange ditutup hub sendiri kalau
    // sudah tidak ada client sama sekali.
    let watch_client_close = async {
        while let Some(msg) = client_read.next().await {
            if matches!(msg, Ok(Message::Close(_)) | Err(_)) {
                break;
            }
        }
    };

    tokio::select! {
        _ = forward_to_client => {},
        _ = watch_client_close => {},
    }
}
