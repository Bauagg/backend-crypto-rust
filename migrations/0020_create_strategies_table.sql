-- Strategi buatan user (indikator / screener / bot) yang logikanya ditulis sebagai script
-- (`strategy_versions`). Baris ini hanya identitas & status-nya; isi script ada di versi, supaya
-- script yang sudah pernah dipakai / di-backtest / dijual tidak pernah berubah diam-diam.
--
-- Kolom jenis/status sengaja VARCHAR tanpa CHECK daftar nilai (nilai yang sah dijaga di kode),
-- supaya jenis baru bisa ditambah tanpa migration:
-- - kind: INDICATOR | SCREENER | BOT — tujuan utama strategi (label untuk user & Store).
-- - visibility: PRIVATE (hanya pemilik) | PUBLIC (bisa dipakai user lain; isi script tetap rahasia).
-- - status: DRAFT | ACTIVE | ARCHIVED.
CREATE TABLE strategies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    name VARCHAR(100) NOT NULL,
    description TEXT,
    kind VARCHAR(30) NOT NULL,
    visibility VARCHAR(20) NOT NULL DEFAULT 'PRIVATE',
    status VARCHAR(20) NOT NULL DEFAULT 'DRAFT',
    -- versi script yang sedang dipakai; FK ditambahkan di migration strategy_versions
    current_version_id UUID,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX idx_strategies_user ON strategies (user_id, created_at DESC) WHERE deleted_at IS NULL;
-- Nama strategi unik per user (tidak peduli huruf besar/kecil).
CREATE UNIQUE INDEX idx_strategies_user_name ON strategies (user_id, lower(name)) WHERE deleted_at IS NULL;
-- Daftar strategi publik (Store / dipakai user lain).
CREATE INDEX idx_strategies_public ON strategies (created_at DESC)
    WHERE visibility = 'PUBLIC' AND deleted_at IS NULL;
