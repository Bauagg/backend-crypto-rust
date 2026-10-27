-- Isi script 1 strategi per versi. IMMUTABLE: tidak pernah di-UPDATE — setiap perubahan script =
-- versi baru (version_no + 1). Performa (backtest, transaksi asli, rating Store) selalu dihitung
-- per versi, jadi track record tidak bisa "dipercantik" dengan mengedit script lama.
CREATE TABLE strategy_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    strategy_id UUID NOT NULL REFERENCES strategies(id),
    version_no INTEGER NOT NULL CHECK (version_no >= 1),
    -- bahasa script; saat ini RHAI (sandbox, dijalankan src/strategy_engine)
    language VARCHAR(20) NOT NULL,
    source_code TEXT NOT NULL,
    -- setelan yang boleh diubah pemakai (seperti input() di Pine Script):
    -- [{"name": "rsi_period", "label": "Periode RSI", "default": 14, "min": 2, "max": 100, "integer": true}]
    inputs JSONB NOT NULL DEFAULT '[]',
    -- bentuk hasil script: SCORE (skor 0–100 + alasan per coin)
    output_type VARCHAR(20) NOT NULL,
    -- data candle yang dipakai script; saat ini hanya 1D (D1)
    timeframe VARCHAR(10) NOT NULL,
    -- sha256 (language + source_code + inputs) — mendeteksi versi yang isinya sama persis
    checksum CHAR(64) NOT NULL,
    change_note TEXT,
    -- percakapan dengan asisten AI yang menghasilkan script ini (kalau dibuat lewat AI) — bahan
    -- analisa/ML: permintaan user -> strategi -> performa
    ai_conversation JSONB,
    created_by VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT strategy_versions_unique_no UNIQUE (strategy_id, version_no)
);

ALTER TABLE strategies
    ADD CONSTRAINT strategies_current_version_fkey
    FOREIGN KEY (current_version_id) REFERENCES strategy_versions(id);
