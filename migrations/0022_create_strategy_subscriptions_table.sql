-- User memakai 1 strategi (miliknya sendiri, atau strategi PUBLIC / dari Store nanti) sebagai
-- SCREENER (mencari coin) atau BOT (order otomatis — tahap berikutnya). Terkunci ke 1 versi
-- (strategy_version_id): versi baru dari pembuat strategi tidak otomatis dipakai sampai user
-- memperbaruinya, jadi perilaku bot tidak berubah diam-diam.
--
-- Transaksi yang dibuka dari strategi dicatat di trade_positions dengan rule_type = 'STRATEGY' dan
-- rule_id = strategy_versions.id -> performa asli per versi strategi.
CREATE TABLE strategy_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    strategy_id UUID NOT NULL REFERENCES strategies(id),
    strategy_version_id UUID NOT NULL REFERENCES strategy_versions(id),
    -- SCREENER | BOT (tanpa CHECK daftar nilai — dijaga di kode)
    usage VARCHAR(20) NOT NULL,
    -- akun tempat bot berjalan (DEMO | LIVE); NULL untuk SCREENER
    account_mode VARCHAR(10),
    -- nilai setelan script (inputs versi) pilihan user, mis. {"rsi_period": 10}
    input_values JSONB NOT NULL DEFAULT '{}',
    -- batas persen modal khusus bot ini (1–100); NULL = ikut batas modal robot user
    capital_percent NUMERIC(5, 2) CHECK (capital_percent >= 1 AND capital_percent <= 100),
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

-- 1 user memakai 1 strategi maksimal sekali per jenis pemakaian.
CREATE UNIQUE INDEX idx_strategy_subscriptions_unique ON strategy_subscriptions (user_id, strategy_id, usage)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_strategy_subscriptions_version ON strategy_subscriptions (strategy_version_id)
    WHERE deleted_at IS NULL;
