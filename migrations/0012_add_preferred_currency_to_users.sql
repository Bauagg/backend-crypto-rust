-- Mata uang pilihan user (DEMO maupun LIVE) — Binance tidak menyediakan info ini lewat API.
-- Dipakai sebagai default mata uang input saat membuka posisi. Saldo (demo_balance) & semua nilai
-- transaksi tetap disimpan dalam USDT; ini hanya preferensi input/tampilan.
ALTER TABLE users
    ADD COLUMN preferred_currency VARCHAR(4) NOT NULL DEFAULT 'USDT'
        CHECK (preferred_currency IN ('IDR', 'USDT'));
