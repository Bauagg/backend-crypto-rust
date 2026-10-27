use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
    pub phone: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub role: String,
    pub status: String,
    pub platform: Option<String>,
    /// Tersimpan terenkripsi — di-decrypt hanya saat request ke exchange. Di response profil
    /// selalu tampil tersamar (`UserProfile`), tidak pernah utuh.
    pub api_key: Option<String>,
    /// Tersimpan terenkripsi — di-decrypt (`utils::crypto::decrypt`) hanya saat menandatangani
    /// request ke exchange (cek saldo LIVE, nanti eksekusi order). Tidak pernah ikut di response.
    #[serde(skip_serializing)]
    pub api_secret: Option<String>,
    pub photo_id: Option<String>,
    pub photo_url: Option<String>,
    /// Saldo akun demo (representasi USD), default 1000, bisa diubah lewat endpoint profile.
    pub demo_balance: Decimal,
    /// Status aktif robot trading di akun demo (simulasi, pakai `demo_balance`).
    pub is_robot_demo_active: bool,
    /// Status aktif robot trading di akun real/platform (pakai `api_key`/`api_secret` exchange).
    pub is_robot_platform_active: bool,
    /// Mata uang pilihan user (`IDR` | `USDT`) — default input saat membuka posisi. Saldo & nilai
    /// transaksi tetap disimpan dalam USDT.
    pub preferred_currency: String,
    /// Modal awal (USDT) robot demo — `modal_awal` untuk kill switch Python. `None` = robot belum
    /// jalan sejak diaktifkan.
    pub demo_initial_capital: Option<Decimal>,
    /// Tanggal (UTC) terakhir robot demo memproses user ini (maks 1x per hari).
    pub demo_robot_last_run_date: Option<chrono::NaiveDate>,
    /// Modal awal (USDT) robot live — `modal_awal` untuk kill switch Python. `None` = robot live
    /// belum jalan sejak diaktifkan.
    pub platform_initial_capital: Option<Decimal>,
    /// Tanggal (UTC) terakhir robot live memproses user ini (maks 1x per hari).
    pub platform_robot_last_run_date: Option<chrono::NaiveDate>,
    /// `true` = user menjual manual coin milik robot demo — robot tidak membeli ulang / menambah
    /// coin sampai rebalance terjadwal strategi ("recovery"). Lihat migration 0017.
    pub demo_robot_awaiting_recovery: bool,
    /// Sama dengan `demo_robot_awaiting_recovery`, untuk robot live.
    pub platform_robot_awaiting_recovery: bool,
    /// Batas persen saldo demo yang boleh dipakai robot (1–100, default 100 = seluruh saldo, sama
    /// dengan aturan bot Python) — lihat migration 0018.
    pub demo_robot_capital_percent: Decimal,
    /// Sama dengan `demo_robot_capital_percent`, untuk akun live.
    pub platform_robot_capital_percent: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 1 permintaan lupa password (tabel `password_resets`). Kode OTP & reset token hanya disimpan
/// sebagai hash SHA-256. Kolom token & `used_at` cukup dicek di query (repository), jadi tidak
/// perlu dibaca ke struct.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PasswordReset {
    pub id: Uuid,
    pub user_id: Uuid,
    pub otp_hash: String,
    pub otp_expires_at: DateTime<Utc>,
    pub attempts: i32,
    pub verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
