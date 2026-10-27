-- Hak pakai strategi Store milik user lain (pembelian / trial). Selama ada baris ACTIVE yang belum
-- kedaluwarsa, user boleh memakai strategi itu sebagai screener/bot.
-- - kind: PURCHASE | TRIAL (trial hanya sekali per user per listing).
-- - status: ACTIVE | EXPIRED | CANCELLED.
-- - payment_status: DUMMY (saat ini — tanpa pembayaran asli, paid_idr = 0) | PAID | FREE.
-- Harga, promo, & potongan dicatat apa adanya supaya saat pembayaran asli aktif datanya sudah lengkap.
CREATE TABLE store_purchases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    listing_id UUID NOT NULL REFERENCES store_listings(id),
    strategy_id UUID NOT NULL REFERENCES strategies(id),
    kind VARCHAR(20) NOT NULL,
    pricing_model VARCHAR(20) NOT NULL,
    list_price_idr NUMERIC(18, 2) NOT NULL,
    discount_idr NUMERIC(18, 2) NOT NULL DEFAULT 0,
    paid_idr NUMERIC(18, 2) NOT NULL DEFAULT 0,
    promo_id UUID REFERENCES store_promos(id),
    payment_status VARCHAR(20) NOT NULL,
    payment_reference VARCHAR(100),
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
    starts_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- NULL = berlaku selamanya (FREE / ONE_TIME)
    expires_at TIMESTAMPTZ,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_store_purchases_user ON store_purchases (user_id, strategy_id, status);
-- Trial hanya sekali per user per listing.
CREATE UNIQUE INDEX idx_store_purchases_one_trial ON store_purchases (user_id, listing_id) WHERE kind = 'TRIAL';
