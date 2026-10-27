-- Backtest lebih lengkap:
-- - is_official: dijalankan pemilik strategi dengan modal 1.000 USDT & setelan default — hanya ini
--   yang dipakai statistik Store (modal backtest sekarang bisa diisi user, jadi perlu ditandai);
-- - validation: uji overfitting (out-of-sample, Monte Carlo, banding acak, sensitivitas setelan);
-- - insights: faktor yang paling memengaruhi untung/rugi trade (seperti feature importance).
ALTER TABLE strategy_backtests
    ADD COLUMN is_official BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN validation JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN insights JSONB NOT NULL DEFAULT '{}';

-- Backtest lama semuanya bermodal 1.000 USDT: yang dijalankan pemilik strategi = resmi.
UPDATE strategy_backtests b
SET is_official = true
FROM strategy_versions v
JOIN strategies s ON s.id = v.strategy_id
WHERE v.id = b.strategy_version_id AND b.user_id = s.user_id AND b.status = 'DONE';

CREATE INDEX idx_strategy_backtests_official
    ON strategy_backtests (strategy_version_id, created_at DESC) WHERE is_official;
