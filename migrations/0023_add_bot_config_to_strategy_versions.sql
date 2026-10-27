-- Aturan robot & cakupan coin per versi strategi (ikut immutable — mengubahnya = versi baru):
-- - entry_score / exit_score: robot BELI coin kalau skor script >= entry_score, JUAL kalau
--   skor <= exit_score (di antaranya = tahan). Ditentukan pembuat saat membuat strategi.
-- - position_percent: persen modal robot untuk 1 coin kalau script tidak menentukan sendiri
--   (script boleh menentukan per coin lewat signal(skor, "alasan", persen)).
-- - max_positions: maksimal coin yang dipegang robot bersamaan.
-- - coin_scope: ALL (semua coin aktif — bisa juga dipakai sebagai SCREENER) | SPECIFIC (hanya
--   coin di `symbols`, mis. {BTCUSDT} — hanya bisa dipakai sebagai BOT).
ALTER TABLE strategy_versions
    ADD COLUMN entry_score NUMERIC(5, 2) NOT NULL DEFAULT 70 CHECK (entry_score BETWEEN 0 AND 100),
    ADD COLUMN exit_score NUMERIC(5, 2) NOT NULL DEFAULT 30 CHECK (exit_score BETWEEN 0 AND 100),
    ADD COLUMN position_percent NUMERIC(5, 2) NOT NULL DEFAULT 20 CHECK (position_percent BETWEEN 1 AND 100),
    ADD COLUMN max_positions SMALLINT NOT NULL DEFAULT 5 CHECK (max_positions BETWEEN 1 AND 50),
    ADD COLUMN coin_scope VARCHAR(20) NOT NULL DEFAULT 'ALL',
    ADD COLUMN symbols TEXT[] NOT NULL DEFAULT '{}',
    ADD CONSTRAINT strategy_versions_exit_below_entry CHECK (exit_score < entry_score);
