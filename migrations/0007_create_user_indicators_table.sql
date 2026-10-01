-- Konfigurasi indikator teknikal yang diaktifkan tiap user untuk robot trading mereka
-- (lihat src/indicators/ untuk daftar fungsi kalkulasi yang sudah tersedia). Satu baris =
-- satu indikator aktif untuk satu user, berlaku ke semua simbol.
CREATE TABLE user_indicators (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    -- Menunjuk ke baris flex_params (type_param = 'INDICATORS') yang mendefinisikan
    -- indikator ini -- daftar indikator valid dikelola lewat flex_params, bukan hardcode di sini.
    flex_param_id UUID NOT NULL REFERENCES flex_params(id),
    -- Disalin dari flex_params.value_param saat baris ini dibuat (mis. 'RSI', 'MACD') supaya
    -- query/tampilan tidak perlu join ke flex_params tiap saat.
    indicator_type VARCHAR(30) NOT NULL,
    -- Parameter spesifik per indikator, bentuknya beda-beda (mis. {"period": 14} untuk RSI,
    -- {"fast_period": 12, "slow_period": 26, "signal_period": 9} untuk MACD) -- JSONB dipakai
    -- daripada kolom terpisah karena tiap indikator punya jumlah & nama parameter berbeda.
    params JSONB NOT NULL DEFAULT '{}',
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,

    CONSTRAINT user_indicators_unique_config UNIQUE (user_id, flex_param_id)
);

CREATE INDEX idx_user_indicators_user_id ON user_indicators (user_id);
