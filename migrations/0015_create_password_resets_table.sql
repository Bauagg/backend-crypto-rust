-- Lupa password: 1 baris = 1 permintaan reset untuk 1 user.
--   1. user minta kode   -> otp_hash + otp_expires_at diisi, kode 6 digit dikirim ke email
--   2. user kirim kode   -> kalau benar: verified_at + reset_token_hash diisi (token dikirim ke FE)
--   3. user kirim password baru + reset_token -> password diganti, used_at diisi
-- Kode OTP & reset token TIDAK disimpan apa adanya — hanya hash SHA-256-nya.
CREATE TABLE password_resets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    otp_hash VARCHAR(64) NOT NULL,
    otp_expires_at TIMESTAMPTZ NOT NULL,
    -- jumlah percobaan kode yang salah — dibatasi supaya kode 6 digit tidak bisa ditebak
    attempts INTEGER NOT NULL DEFAULT 0,
    verified_at TIMESTAMPTZ,
    reset_token_hash VARCHAR(64),
    reset_token_expires_at TIMESTAMPTZ,
    -- selesai dipakai (password sudah diganti) atau dibatalkan karena ada permintaan baru
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- permintaan terbaru user yang masih aktif
CREATE INDEX idx_password_resets_user_active ON password_resets (user_id, created_at DESC)
    WHERE used_at IS NULL;

CREATE UNIQUE INDEX idx_password_resets_token ON password_resets (reset_token_hash)
    WHERE reset_token_hash IS NOT NULL;
