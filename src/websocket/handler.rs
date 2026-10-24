//! Endpoint WebSocket untuk FE + relay pesan dari hub ke client.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::Query;
use axum::response::Response;
use axum::Extension;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use super::kline_hub::KlineHub;
use super::ticker_hub::TickerHub;
use super::types::{MarketTicker, WsKlineQuery, WsTickersQuery, WsTickersRequest};
use crate::services::market::types::is_valid_interval;
use crate::utils::app_error::AppError;

/// Maksimal simbol yang dipantau 1 client — sama dengan batas `limit` halaman daftar coin.
const MAX_WATCHED_SYMBOLS: usize = 100;

/// Upgrade koneksi HTTP jadi WebSocket, lalu kirim candle live ke client lewat `KlineHub`
/// (1 koneksi ke exchange per simbol+interval, dipakai bersama semua client).
/// Contoh: GET /api/market/ws?symbol=BTCUSDT&interval=1m (upgrade header WebSocket).
pub async fn ws_klines(
    ws: WebSocketUpgrade,
    Extension(hub): Extension<Arc<KlineHub>>,
    Query(query): Query<WsKlineQuery>,
) -> Result<Response, AppError> {
    if query.symbol.trim().is_empty() {
        return Err(AppError::BadRequest("symbol wajib diisi".to_string()));
    }

    let interval = query.interval.unwrap_or_else(|| "1m".to_string());
    if !is_valid_interval(&interval) {
        return Err(AppError::BadRequest("interval tidak valid".to_string()));
    }

    let symbol = query.symbol.trim().to_uppercase();

    Ok(ws.on_upgrade(move |socket| relay_kline_stream(socket, hub, symbol, interval)))
}

/// Upgrade ke WebSocket harga live daftar coin (`TickerHub`: 1 koneksi exchange untuk semua coin
/// & semua client). Simbol awal opsional lewat query; ganti kapan saja dengan pesan
/// `{"symbols":["BTCUSDT","ETHUSDT"]}`.
/// Contoh: GET /api/market/ws/tickers?symbols=BTCUSDT,ETHUSDT (upgrade header WebSocket).
pub async fn ws_tickers(
    ws: WebSocketUpgrade,
    Extension(hub): Extension<Arc<TickerHub>>,
    Query(query): Query<WsTickersQuery>,
) -> Response {
    let initial: Vec<String> = query
        .symbols
        .unwrap_or_default()
        .split(',')
        .map(str::to_string)
        .collect();

    ws.on_upgrade(move |socket| relay_ticker_stream(socket, hub, initial))
}

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

fn normalize_symbols(symbols: impl IntoIterator<Item = String>) -> HashSet<String> {
    symbols
        .into_iter()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .take(MAX_WATCHED_SYMBOLS)
        .collect()
}

/// Kirim harga live coin yang sedang tampil di daftar FE. Client mengganti daftar simbolnya
/// kapan saja (mis. setelah scroll) dengan `{"symbols":[...]}` lewat koneksi yang sama; balasannya
/// langsung harga terakhir simbol-simbol itu, lalu tiap detik hanya yang harganya berubah.
/// Bentuk pesan: array JSON `MarketTicker`. Semua client berbagi 1 koneksi exchange (`TickerHub`).
pub async fn relay_ticker_stream(client_socket: WebSocket, hub: Arc<TickerHub>, initial: Vec<String>) {
    let mut updates = hub.subscribe();
    let (mut client_write, mut client_read) = client_socket.split();
    let mut watched = normalize_symbols(initial);

    let snapshot = |watched: &HashSet<String>| -> Vec<MarketTicker> {
        let symbols: Vec<String> = watched.iter().cloned().collect();
        hub.get(&symbols).into_values().collect()
    };
    let to_message = |items: &[MarketTicker]| serde_json::to_string(items).ok().map(Message::Text);

    let first = snapshot(&watched);
    if let Some(msg) = (!first.is_empty()).then(|| to_message(&first)).flatten()
        && client_write.send(msg).await.is_err() {
            return;
        }

    loop {
        let outgoing = tokio::select! {
            update = updates.recv() => match update {
                Ok(batch) => {
                    let items: Vec<MarketTicker> =
                        batch.iter().filter(|t| watched.contains(&t.symbol)).cloned().collect();
                    items
                }
                // Client terlalu lambat & melewatkan batch lama — batch berikutnya tetap menyusul.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            msg = client_read.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    // Pesan yang bukan `{"symbols":[...]}` diabaikan, koneksi tetap jalan.
                    let Ok(request) = serde_json::from_str::<WsTickersRequest>(&text) else { continue };
                    watched = normalize_symbols(request.symbols);
                    snapshot(&watched)
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => continue,
            },
        };

        if outgoing.is_empty() {
            continue;
        }
        let Some(msg) = to_message(&outgoing) else { continue };
        if client_write.send(msg).await.is_err() {
            break;
        }
    }
}
