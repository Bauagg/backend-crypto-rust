//! Klien 9router — gateway AI dengan API format OpenAI (`POST /v1/chat/completions`). Model &
//! urutan fallback-nya diatur di dashboard 9router lewat "combo" (`AI_ROUTER_MODEL`), bukan di kode.
//!
//! 9router membalas streaming (SSE) kalau tidak dilarang, jadi `stream: false` selalu dikirim.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::utils::app_error::AppError;
use crate::utils::http_client::{post_json_with_error_body, ErrorResponse};

/// Batas panjang jawaban AI (token) per request.
const MAX_TOKENS: u32 = 2000;

/// 1 pesan percakapan: `role` = `system` | `user` | `assistant`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self { role: role.to_string(), content: content.into() }
    }
}

#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn not_configured() -> AppError {
    tracing::error!("AI_ROUTER_BASE_URL / AI_ROUTER_API_KEY / AI_ROUTER_MODEL belum diisi di .env");
    AppError::Internal("Asisten AI belum dikonfigurasi".to_string())
}

/// Status error 9router -> pesan untuk user. Masalah konfigurasi (key salah) cukup di-log.
fn to_app_error(error: ErrorResponse) -> AppError {
    match error.status {
        401 | 403 => {
            tracing::error!("9router menolak API key ({}): {} — cek AI_ROUTER_API_KEY", error.status, error.body);
            AppError::Internal("Asisten AI sedang bermasalah".to_string())
        }
        429 => AppError::BadRequest("Asisten AI sedang sibuk, coba lagi sebentar".to_string()),
        status => {
            tracing::error!("9router error {status}: {}", error.body);
            AppError::Internal("Asisten AI sedang bermasalah, coba lagi sebentar".to_string())
        }
    }
}

/// Kirim percakapan, balik isi jawaban AI (teks).
pub async fn chat(messages: &[ChatMessage]) -> Result<String, AppError> {
    let base_url = env("AI_ROUTER_BASE_URL").ok_or_else(not_configured)?;
    let api_key = env("AI_ROUTER_API_KEY").ok_or_else(not_configured)?;
    let model = env("AI_ROUTER_MODEL").ok_or_else(not_configured)?;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "stream": false,
        "max_tokens": MAX_TOKENS,
        "messages": messages,
    });
    let headers = vec![("Authorization", format!("Bearer {api_key}"))];

    let completion: Completion = post_json_with_error_body(&url, headers, &body)
        .await?
        .map_err(to_app_error)?;
    completion
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| AppError::Internal("Asisten AI tidak memberi jawaban, coba lagi".to_string()))
}
