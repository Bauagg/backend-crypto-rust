-- State robot akun LIVE (Binance asli) per user — padanan kolom robot demo di 0013:
-- - platform_initial_capital: modal awal (USDT) saat robot live mulai jalan — dikirim ke /signal
--   Python sebagai `modal_awal` (dasar kill switch). NULL = belum pernah jalan sejak diaktifkan;
--   diisi robot di putaran pertamanya, dikosongkan lagi saat user mengaktifkan ulang robot.
-- - platform_robot_last_run_date: tanggal (UTC) terakhir robot live memproses user ini — supaya
--   order ke Binance maksimal dikirim 1x per hari walau server restart.
ALTER TABLE users
    ADD COLUMN platform_initial_capital NUMERIC(28, 8),
    ADD COLUMN platform_robot_last_run_date DATE;
