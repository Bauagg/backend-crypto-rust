-- Cache histori Fear & Greed Index (sentimen pasar crypto, 0-100) dari alternative.me,
-- dipakai sebagai salah satu fitur input model ML strategi V5 (backtes-crypto).
CREATE TABLE fear_greed_index (
    date DATE PRIMARY KEY,
    fng_value SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
