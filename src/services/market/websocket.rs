use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

use super::types::{Candle, RawKlineEvent};

fn ws_base_url() -> String {
    std::env::var("MARKET_WS_BASE_URL")
        .unwrap_or_else(|_| "wss://stream-cloud.tokocrypto.site/ws".to_string())
}

/// Relay kline stream Tokocrypto (candle yang sedang berjalan, update live) ke client kita.
/// Tiap pesan Tokocrypto diparse ulang jadi `Candle` — bentuk JSON yang sama persis dengan
/// endpoint REST /klines — supaya frontend cuma perlu satu struktur data untuk histori & live.
pub async fn relay_kline_stream(mut client_socket: WebSocket, symbol: String, interval: String) {
    let symbol = symbol.to_lowercase();
    let base_url = ws_base_url();
    let stream_url = format!("{base_url}/{symbol}@kline_{interval}");

    let (upstream, _) = match tokio_tungstenite::connect_async(&stream_url).await {
        Ok(conn) => conn,
        Err(err) => {
            tracing::error!("Gagal connect ke Tokocrypto WebSocket: {err}");
            let _ = client_socket
                .send(Message::Text(
                    r#"{"error":"Gagal menghubungkan ke Tokocrypto"}"#.to_string(),
                ))
                .await;
            return;
        }
    };

    let (mut upstream_write, mut upstream_read) = upstream.split();
    let (mut client_write, mut client_read) = client_socket.split();

    // parse tiap kline event Tokocrypto -> Candle, lalu teruskan ke client kita
    let forward_to_client = async {
        while let Some(msg) = upstream_read.next().await {
            match msg {
                Ok(UpstreamMessage::Text(text)) => {
                    let Ok(event) = serde_json::from_str::<RawKlineEvent>(&text) else {
                        continue;
                    };
                    let candle = Candle::from(event);
                    let Ok(payload) = serde_json::to_string(&candle) else {
                        continue;
                    };
                    if client_write.send(Message::Text(payload)).await.is_err() {
                        break;
                    }
                }
                Ok(UpstreamMessage::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    };

    // kalau client disconnect / kirim close, putus juga koneksi ke upstream
    let watch_client_close = async {
        while let Some(msg) = client_read.next().await {
            if matches!(msg, Ok(Message::Close(_)) | Err(_)) {
                break;
            }
        }
        let _ = upstream_write.send(UpstreamMessage::Close(None)).await;
    };

    tokio::select! {
        _ = forward_to_client => {},
        _ = watch_client_close => {},
    }
}
