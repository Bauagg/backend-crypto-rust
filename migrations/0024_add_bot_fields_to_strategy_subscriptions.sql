-- Pemakaian strategi sebagai BOT bisa di DEMO dan LIVE sekaligus (2 baris terpisah, masing-masing
-- dengan persen modal sendiri) — index unik lama (user, strategi, usage) diganti supaya
-- account_mode ikut dibedakan.
-- - last_run_date: tanggal (UTC) terakhir worker menjalankan bot ini — order maksimal 1x per hari
--   walau worker dicoba ulang / server restart.
ALTER TABLE strategy_subscriptions ADD COLUMN last_run_date DATE;

DROP INDEX idx_strategy_subscriptions_unique;
CREATE UNIQUE INDEX idx_strategy_subscriptions_unique
    ON strategy_subscriptions (user_id, strategy_id, usage, COALESCE(account_mode, ''))
    WHERE deleted_at IS NULL;

-- Daftar bot yang dijalankan worker tiap hari.
CREATE INDEX idx_strategy_subscriptions_active_bots ON strategy_subscriptions (account_mode)
    WHERE usage = 'BOT' AND is_active AND deleted_at IS NULL;
