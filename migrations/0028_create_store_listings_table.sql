-- Strategi yang dipasang pembuatnya di "Store Indicator Bot". 1 strategi maksimal 1 listing.
-- Isi script tidak pernah dikirim ke pembeli — mereka hanya bisa memakainya (screener/bot).
-- - pricing_model: FREE | ONE_TIME (sekali bayar, berlaku selamanya) | MONTHLY (per 30 hari).
-- - price_idr: harga dari pembuat. Saat ini pembayaran masih dummy — pembeli membayar Rp0
--   (store_purchases.paid_idr), harga & promo tetap dicatat untuk saat pembayaran asli aktif.
-- - trial_days: lama masa coba gratis dari pembuat (0 = tanpa trial).
-- - status: ACTIVE (tampil & bisa dibeli) | INACTIVE (disembunyikan pembuat).
CREATE TABLE store_listings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    strategy_id UUID NOT NULL REFERENCES strategies(id),
    -- pembuat (pemilik strategi) — ditampilkan dengan nama aslinya
    seller_user_id UUID NOT NULL REFERENCES users(id),
    title VARCHAR(120) NOT NULL,
    description TEXT,
    pricing_model VARCHAR(20) NOT NULL,
    price_idr NUMERIC(18, 2) NOT NULL DEFAULT 0 CHECK (price_idr >= 0),
    trial_days SMALLINT NOT NULL DEFAULT 0 CHECK (trial_days BETWEEN 0 AND 90),
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
    published_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX idx_store_listings_strategy ON store_listings (strategy_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_store_listings_browse ON store_listings (published_at DESC)
    WHERE status = 'ACTIVE' AND deleted_at IS NULL;
