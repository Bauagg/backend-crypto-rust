-- Tiap trade simulasi 1 backtest (1 baris = beli -> jual 1 coin). Bahan analisa/ML: skor &
-- alasan saat masuk/keluar vs hasilnya.
CREATE TABLE strategy_backtest_trades (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    backtest_id UUID NOT NULL REFERENCES strategy_backtests(id) ON DELETE CASCADE,
    symbol VARCHAR(20) NOT NULL,
    entry_date DATE NOT NULL,
    -- NULL = masih dipegang saat backtest berakhir (dinilai dengan harga close terakhir)
    exit_date DATE,
    entry_price NUMERIC(28, 8) NOT NULL,
    exit_price NUMERIC(28, 8) NOT NULL,
    quantity NUMERIC(28, 8) NOT NULL,
    entry_value NUMERIC(28, 8) NOT NULL,
    exit_value NUMERIC(28, 8) NOT NULL,
    fee_amount NUMERIC(28, 8) NOT NULL,
    pnl_amount NUMERIC(28, 8) NOT NULL,
    pnl_percent NUMERIC(12, 4) NOT NULL,
    holding_days INTEGER NOT NULL,
    entry_score NUMERIC(6, 2) NOT NULL,
    exit_score NUMERIC(6, 2),
    entry_reason TEXT,
    exit_reason TEXT
);

CREATE INDEX idx_strategy_backtest_trades_backtest ON strategy_backtest_trades (backtest_id, entry_date);
