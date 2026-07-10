-- Data OHLCV historis (candle yang sudah CLOSED saja) untuk keperluan analisis & training ML.
-- Diisi otomatis oleh background collector yang subscribe ke stream Tokocrypto per simbol+interval.
CREATE TABLE market_candles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol VARCHAR(20) NOT NULL,
    interval VARCHAR(5) NOT NULL,
    open_time BIGINT NOT NULL,
    close_time BIGINT NOT NULL,
    open NUMERIC(28, 8) NOT NULL,
    high NUMERIC(28, 8) NOT NULL,
    low NUMERIC(28, 8) NOT NULL,
    close NUMERIC(28, 8) NOT NULL,
    volume NUMERIC(28, 8) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- satu candle final per kombinasi simbol+interval+waktu buka, tidak pernah diduplikasi
    CONSTRAINT market_candles_unique_candle UNIQUE (symbol, interval, open_time)
);

-- index untuk query histori per simbol+interval terurut waktu (pola akses paling umum untuk ML/backtest)
CREATE INDEX idx_market_candles_symbol_interval_time
    ON market_candles (symbol, interval, open_time);
