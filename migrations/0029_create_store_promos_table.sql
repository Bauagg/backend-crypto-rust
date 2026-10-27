-- Kode promo yang dibuat pembuat strategi untuk listing-nya: potongan persen dalam rentang waktu,
-- dengan batas jumlah pemakaian opsional.
CREATE TABLE store_promos (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    listing_id UUID NOT NULL REFERENCES store_listings(id),
    code VARCHAR(30) NOT NULL,
    discount_percent NUMERIC(5, 2) NOT NULL CHECK (discount_percent > 0 AND discount_percent <= 100),
    starts_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- NULL = tanpa batas akhir
    ends_at TIMESTAMPTZ,
    -- NULL = tanpa batas jumlah
    max_redemptions INTEGER CHECK (max_redemptions > 0),
    redeemed_count INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,

    CONSTRAINT store_promos_period CHECK (ends_at IS NULL OR ends_at > starts_at)
);

-- Kode promo unik per listing (tidak peduli huruf besar/kecil).
CREATE UNIQUE INDEX idx_store_promos_code ON store_promos (listing_id, upper(code)) WHERE deleted_at IS NULL;
