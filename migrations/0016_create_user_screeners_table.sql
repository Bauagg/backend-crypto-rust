-- Strategi screening milik user: kumpulan indikator + kondisi (`user_screener_rules`) yang dipakai
-- menilai semua coin dari candle harian (D1). Hasil screening TIDAK disimpan — dihitung saat user
-- membuka hasilnya; yang disimpan hanya strateginya. 1 user hanya punya 1 strategi aktif:
-- menyimpan strategi baru = mengganti isi strategi yang ada.
CREATE TABLE user_screeners (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    name VARCHAR(100) NOT NULL,
    description TEXT,
    -- jumlah coin skor tertinggi yang ditampilkan
    max_results SMALLINT NOT NULL DEFAULT 10 CHECK (max_results BETWEEN 7 AND 20),
    -- coin dengan skor di bawah ini tidak ditampilkan (0 = tampilkan semua sampai max_results)
    min_score SMALLINT NOT NULL DEFAULT 0 CHECK (min_score BETWEEN 0 AND 100),
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

-- 1 strategi aktif per user (yang sudah dihapus tidak dihitung) — juga target `ON CONFLICT` upsert.
CREATE UNIQUE INDEX idx_user_screeners_one_per_user ON user_screeners (user_id)
    WHERE deleted_at IS NULL;

-- 1 baris = 1 indikator di dalam strategi. Indikator yang sama boleh dipakai lebih dari sekali
-- (mis. SMA 50 & SMA 200), jadi tidak ada UNIQUE (screener_id, flex_param_id).
CREATE TABLE user_screener_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    screener_id UUID NOT NULL REFERENCES user_screeners(id),
    -- indikator (flex_params INDICATORS)
    flex_param_id UUID NOT NULL REFERENCES flex_params(id),
    -- salinan flex_params.value_param (mis. RSI) — dicocokkan ke rumus di src/indicator/
    indicator_code VARCHAR(30) NOT NULL,
    -- setelan indikator + batas kondisi, bentuknya beda per indikator, mis. {"period": 13, "threshold": 30}
    params JSONB NOT NULL DEFAULT '{}',
    -- kondisi yang dicari (preset per indikator), mis. BELOW, CROSS_UP, PRICE_ABOVE
    condition VARCHAR(50) NOT NULL,
    -- bobot terhadap skor total
    weight SMALLINT NOT NULL DEFAULT 1 CHECK (weight BETWEEN 1 AND 5),
    sort_order SMALLINT NOT NULL DEFAULT 0,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX idx_user_screener_rules_screener ON user_screener_rules (screener_id, sort_order)
    WHERE deleted_at IS NULL;
