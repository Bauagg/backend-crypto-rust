-- Aturan yang menghasilkan posisi — bahan analisa strategi mana yang profit & stabil:
-- - rule_type: jenis aturan, saat ini ROBOT (dibuka robot, /signal Python) | MANUAL (beli manual
--   biasa) | SCREENER (beli manual dari hasil strategi screener user) | RECOMMENDATION (beli manual
--   dari rekomendasi coin Python) | STRATEGY (dari hasil strategi script buatan user). Sengaja tanpa
--   CHECK daftar nilai — jenis aturan akan bertambah; nilai yang sah dijaga di kode.
-- - rule_id: id aturannya — SCREENER = user_screeners.id, STRATEGY = strategy_versions.id (performa
--   per versi script); NULL untuk ROBOT, MANUAL, dan RECOMMENDATION (strategi rekomendasi milik
--   Python, tidak tersimpan di DB ini). Tanpa
--   foreign key karena tabel tujuannya tergantung rule_type. Rule screener yang aktif saat posisi
--   dibuka bisa dilacak dari user_screener_rules milik screener itu dengan
--   created_at <= opened_at < deleted_at (rule lama di-soft delete, tidak hilang).
-- Baris pecahan saat SELL sebagian ikut membawa rule_type & rule_id posisi asalnya.
ALTER TABLE trade_positions
    ADD COLUMN rule_type VARCHAR(30) NOT NULL DEFAULT 'MANUAL',
    ADD COLUMN rule_id UUID;

-- DEFAULT sengaja dipertahankan: server yang masih menjalankan kode lama (belum tahu kolom ini,
-- mis. server lain yang memakai DB yang sama) tetap bisa insert posisi — tidak gagal mencatat
-- order yang sudah terisi di exchange. Kode baru selalu mengisi rule_type sendiri.
UPDATE trade_positions SET rule_type = 'ROBOT' WHERE source = 'BOT';

-- Analisa per jenis aturan / per strategi (mis. semua posisi dari 1 screener).
CREATE INDEX idx_trade_positions_rule ON trade_positions (rule_type, rule_id);
