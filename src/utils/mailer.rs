//! Kirim email lewat SMTP — konfigurasi dari env `SMTP_*` (lihat `.env.example`). Koneksi SMTP
//! dibuat sekali lalu dipakai ulang (pool). Port 465 = TLS langsung, selain itu (587) = STARTTLS.

use std::sync::OnceLock;

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::utils::app_error::AppError;

use crate::template::{APP_NAME as DEFAULT_FROM_NAME, LOGO_CID, LOGO_PNG};

struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

static MAILER: OnceLock<Mailer> = OnceLock::new();

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn not_configured(key: &str) -> AppError {
    AppError::Internal(format!("Pengiriman email belum dikonfigurasi ({key} kosong di .env)"))
}

fn load() -> Result<Mailer, AppError> {
    let host = env("SMTP_HOST").ok_or_else(|| not_configured("SMTP_HOST"))?;
    let username = env("SMTP_USERNAME").ok_or_else(|| not_configured("SMTP_USERNAME"))?;
    // App Password Google kadang disalin dengan spasi ("abcd efgh ...") — spasinya dibuang.
    let password = env("SMTP_PASSWORD")
        .map(|p| p.replace(' ', ""))
        .ok_or_else(|| not_configured("SMTP_PASSWORD"))?;
    let port: u16 = env("SMTP_PORT").and_then(|p| p.parse().ok()).unwrap_or(587);
    let from_email = env("SMTP_FROM_EMAIL").unwrap_or_else(|| username.clone());
    let from_name = env("SMTP_FROM_NAME").unwrap_or_else(|| DEFAULT_FROM_NAME.to_string());

    let builder = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
    }
    .map_err(|err| AppError::Internal(format!("Konfigurasi SMTP_HOST tidak valid: {err}")))?;

    let transport = builder
        .port(port)
        .credentials(Credentials::new(username, password))
        .build();
    let from = format!("{from_name} <{from_email}>")
        .parse::<Mailbox>()
        .map_err(|err| AppError::Internal(format!("SMTP_FROM_EMAIL tidak valid: {err}")))?;

    Ok(Mailer { transport, from })
}

fn mailer() -> Result<&'static Mailer, AppError> {
    if let Some(mailer) = MAILER.get() {
        return Ok(mailer);
    }
    let mailer = load()?;
    Ok(MAILER.get_or_init(|| mailer))
}

/// Kirim 1 email berisi versi HTML (tampilan) + teks biasa (untuk aplikasi email yang tidak
/// menampilkan HTML). HTML yang memakai `cid:` logo otomatis dilampiri logo sebagai gambar inline.
pub async fn send_email(to: &str, subject: &str, text: String, html: String) -> Result<(), AppError> {
    let mailer = mailer()?;
    let to = to
        .parse::<Mailbox>()
        .map_err(|_| AppError::BadRequest("Alamat email tujuan tidak valid".to_string()))?;

    let message = Message::builder()
        .from(mailer.from.clone())
        .to(to)
        .subject(subject)
        .multipart(body_of(text, html))
        .map_err(|err| AppError::Internal(format!("Gagal menyusun email: {err}")))?;

    mailer.transport.send(message).await.map_err(|err| {
        tracing::error!("Gagal mengirim email '{subject}': {err}");
        AppError::Internal("Gagal mengirim email, coba lagi beberapa saat".to_string())
    })?;
    Ok(())
}

/// teks | (HTML + logo inline kalau HTML memakai `cid:` logo).
fn body_of(text: String, html: String) -> MultiPart {
    let uses_logo = html.contains(&format!("cid:{LOGO_CID}"));
    let mut html_part = MultiPart::related().singlepart(SinglePart::html(html));
    if uses_logo {
        let logo = Attachment::new_inline(LOGO_CID.to_string())
            .body(LOGO_PNG.to_vec(), ContentType::parse("image/png").expect("image/png valid"));
        html_part = html_part.singlepart(logo);
    }
    MultiPart::alternative().singlepart(SinglePart::plain(text)).multipart(html_part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_ikut_sebagai_gambar_inline() {
        let mime = String::from_utf8(
            body_of("teks".into(), format!(r#"<img src="cid:{LOGO_CID}">"#)).formatted(),
        )
        .unwrap();
        assert!(mime.contains(&format!("Content-ID: <{LOGO_CID}>")));
        assert!(mime.contains("Content-Type: image/png") && mime.contains("Content-Disposition: inline"));

        let tanpa_logo = String::from_utf8(body_of("teks".into(), "<p>hai</p>".into()).formatted()).unwrap();
        assert!(!tanpa_logo.contains("image/png"));
    }
}
