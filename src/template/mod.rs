//! Template email yang dipakai ulang lintas modul. Tampilan (HTML) ada di sini; kata-katanya
//! dikirim pemanggil lewat parameter — jadi 1 template bisa untuk reset password, login OTP,
//! verifikasi email, dst. tanpa menyalin HTML.
//!
//! HTML pakai tabel + CSS inline (bukan `<style>`/flexbox) supaya tampil sama di Gmail, Outlook,
//! dan aplikasi email HP. Semua teks dari parameter di-escape sebelum masuk HTML.

mod otp_email;

pub use otp_email::OtpEmail;

/// Nama aplikasi di header & footer email.
pub const APP_NAME: &str = "Algo Bot PRO";

/// Logo di header email (salinan kecil `aset/icon.png`, 192px), ikut di dalam binary. Dikirim
/// sebagai gambar inline (`cid:`) — Gmail dkk. memblokir gambar base64 & sering menunda gambar
/// dari URL luar.
pub const LOGO_PNG: &[u8] = include_bytes!("logo.png");
/// Content-ID logo; di HTML dipakai sebagai `src="cid:{LOGO_CID}"`.
pub const LOGO_CID: &str = "algobot-logo";

/// Hasil render: subjek, versi teks biasa (aplikasi email tanpa HTML), dan versi HTML.
pub struct RenderedEmail {
    pub subject: String,
    pub text: String,
    pub html: String,
}

/// Escape teks sebelum disisipkan ke HTML (nama user, pesan, dll.).
pub(crate) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
