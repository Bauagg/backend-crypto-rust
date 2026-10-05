-- Posisi trading per user: 1 baris = 1 siklus beli -> jual satu coin (dibuka saat beli, ditutup
-- saat jual). Dipakai untuk akun LIVE (Binance asli) maupun DEMO (simulasi), baik dibuka robot
-- maupun manual oleh user.
CREATE TABLE trade_positions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    -- snapshot email pemilik posisi (tetap terbaca walau email user berubah / untuk laporan)
    user_email VARCHAR(255) NOT NULL,
    -- coin yang diperdagangkan (flex_params SIMBOL_CRYPTO)
    flex_param_id UUID NOT NULL REFERENCES flex_params(id),
    -- snapshot simbol saat posisi dibuka, mis. BTCUSDT
    symbol VARCHAR(20) NOT NULL,
    -- siapa yang membuka posisi: robot (/signal Python) atau user manual
    source VARCHAR(10) NOT NULL CHECK (source IN ('BOT', 'MANUAL')),
    -- strategi Python yang menghasilkan posisi (mis. V23, BTC-60); NULL untuk posisi manual
    strategy VARCHAR(20),
    opened_at TIMESTAMPTZ NOT NULL,
    -- NULL selama posisi masih OPEN
    closed_at TIMESTAMPTZ,
    holding_days INTEGER NOT NULL DEFAULT 0,
    -- mata uang yang dipakai user saat input. Semua kolom nilai (entry_value, exit_value,
    -- fee_amount, pnl_amount, account_capital) tetap disimpan dalam USDT; konversi ke IDR pakai
    -- usdt_idr_rate.
    currency VARCHAR(4) NOT NULL CHECK (currency IN ('IDR', 'USDT')),
    -- kurs 1 USDT dalam Rupiah dari Binance (pair USDTIDR) saat posisi dibuka
    usdt_idr_rate NUMERIC(20, 4) NOT NULL CHECK (usdt_idr_rate > 0),
    -- jumlah coin yang dipegang (qty asli setelah fee) — dikirim ke /signal sebagai `posisi`
    quantity NUMERIC(28, 8) NOT NULL CHECK (quantity >= 0),
    -- USDT yang dipakai saat masuk & yang diterima saat posisi ditutup (NULL selama OPEN)
    entry_value NUMERIC(28, 8) NOT NULL,
    exit_value NUMERIC(28, 8),
    -- harga aktual simbol saat beli & saat posisi ditutup (presisi sama dengan candle_ohlcv);
    -- exit_price NULL selama posisi masih OPEN
    entry_price NUMERIC(28, 8) NOT NULL,
    exit_price NUMERIC(28, 8),
    -- total fee exchange (beli + jual) dalam USDT
    fee_amount NUMERIC(28, 8) NOT NULL CHECK (fee_amount >= 0),
    -- orderId Binance saat beli & jual — NULL untuk akun DEMO / posisi yang belum ditutup
    entry_order_id VARCHAR(50),
    exit_order_id VARCHAR(50),
    -- bukti order berhasil di Binance (akun LIVE): respons asli POST /api/v3/order apa adanya
    -- (orderId, status FILLED, transactTime, executedQty, fills + commission). NULL untuk DEMO /
    -- posisi manual / posisi yang belum ditutup.
    entry_order_response JSONB,
    exit_order_response JSONB,
    -- untung/rugi dalam USDT dan persen (final saat CLOSED)
    pnl_amount NUMERIC(28, 8) NOT NULL DEFAULT 0,
    pnl_percent NUMERIC(12, 4) NOT NULL DEFAULT 0,
    status VARCHAR(10) NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN', 'CLOSED')),
    -- NULL selama posisi masih OPEN (hasil belum ada)
    result VARCHAR(10) CHECK (result IN ('PROFIT', 'LOSS')),
    -- total modal akun saat posisi dibuka
    account_capital NUMERIC(28, 8) NOT NULL,
    account_mode VARCHAR(10) NOT NULL CHECK (account_mode IN ('LIVE', 'DEMO')),
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,

    -- posisi OPEN belum punya data penutupan; posisi CLOSED wajib punya waktu tutup, harga & nilai
    -- keluar, dan hasil
    CONSTRAINT trade_positions_status_consistent CHECK (
        (status = 'OPEN' AND closed_at IS NULL AND exit_price IS NULL AND exit_value IS NULL
            AND exit_order_id IS NULL AND exit_order_response IS NULL AND result IS NULL)
        OR (status = 'CLOSED' AND closed_at IS NOT NULL AND exit_price IS NOT NULL
            AND exit_value IS NOT NULL AND result IS NOT NULL)
    ),
    -- posisi robot selalu punya strategi
    CONSTRAINT trade_positions_bot_has_strategy CHECK (source = 'MANUAL' OR strategy IS NOT NULL),
    -- bukti order hanya ada di akun LIVE, dan selalu berpasangan dengan orderId-nya
    CONSTRAINT trade_positions_order_proof_live CHECK (
        (entry_order_response IS NULL AND exit_order_response IS NULL)
        OR account_mode = 'LIVE'
    ),
    CONSTRAINT trade_positions_order_proof_has_id CHECK (
        (entry_order_response IS NULL OR entry_order_id IS NOT NULL)
        AND (exit_order_response IS NULL OR exit_order_id IS NOT NULL)
    )
);

-- posisi yang sedang terbuka milik user (dicek bot tiap hari)
CREATE INDEX idx_trade_positions_user_status ON trade_positions (user_id, status)
    WHERE deleted_at IS NULL;

-- riwayat posisi user terbaru dulu (log trade / dashboard)
CREATE INDEX idx_trade_positions_user_opened ON trade_positions (user_id, opened_at DESC)
    WHERE deleted_at IS NULL;
