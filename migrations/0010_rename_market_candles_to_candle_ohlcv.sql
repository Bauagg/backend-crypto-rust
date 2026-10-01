-- Samakan nama tabel dengan modul `candle_ohlcv` (sebelumnya `candle_market`).
ALTER TABLE market_candles RENAME TO candle_ohlcv;
ALTER TABLE candle_ohlcv RENAME CONSTRAINT market_candles_pkey TO candle_ohlcv_pkey;
ALTER TABLE candle_ohlcv RENAME CONSTRAINT market_candles_unique_candle TO candle_ohlcv_unique_candle;
ALTER INDEX idx_market_candles_symbol_interval_time RENAME TO idx_candle_ohlcv_symbol_interval_time;
