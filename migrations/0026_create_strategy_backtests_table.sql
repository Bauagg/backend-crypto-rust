-- Hasil uji 1 versi strategi ke histori candle harian (D1) yang tersimpan — simulasi robot dengan
-- aturan yang sama dengan bot sungguhan (skor masuk/keluar, ukuran posisi, maks posisi, fee 0,1%),
-- sinyal dari candle close hari D dieksekusi di harga open hari D+1 (tanpa melihat masa depan).
-- Hasilnya disimpan (tidak dihitung ulang tiap dibuka) — bahan statistik Store & analisa ML.
CREATE TABLE strategy_backtests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    strategy_version_id UUID NOT NULL REFERENCES strategy_versions(id),
    -- user yang menjalankan backtest
    user_id UUID NOT NULL REFERENCES users(id),
    -- setelan script yang dipakai (hasil resolve inputs)
    input_values JSONB NOT NULL DEFAULT '{}',
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    initial_capital NUMERIC(28, 8) NOT NULL,
    fee_rate NUMERIC(10, 6) NOT NULL,
    -- DONE | FAILED (tanpa CHECK daftar nilai — dijaga di kode)
    status VARCHAR(20) NOT NULL,
    -- ringkasan: return, CAGR, max drawdown, win rate, profit factor, Sharpe/Sortino, vs BTC, dll.
    stats JSONB NOT NULL DEFAULT '{}',
    -- uji ketahanan: performa paruh awal vs akhir, bulan profit, drawdown terlama, label KUAT/SEDANG/LEMAH
    robustness JSONB NOT NULL DEFAULT '{}',
    -- kurva modal harian: [{"date", "equity", "drawdown_percent", "btc_equity"}]
    equity_curve JSONB NOT NULL DEFAULT '[]',
    -- kelebihan & kelemahan dalam bahasa sederhana (dirangkum dari statistik)
    analysis JSONB NOT NULL DEFAULT '[]',
    error TEXT,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_by VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_strategy_backtests_version ON strategy_backtests (strategy_version_id, created_at DESC);
