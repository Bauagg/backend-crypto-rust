-- State robot demo per user:
-- - demo_initial_capital: modal awal (USDT) saat robot demo mulai jalan — dikirim ke /signal Python
--   sebagai `modal_awal` (dasar kill switch). NULL = belum pernah jalan sejak diaktifkan; diisi
--   robot di putaran pertamanya, dan dikosongkan lagi saat user mengaktifkan ulang robot.
-- - demo_robot_last_run_date: tanggal (UTC) terakhir robot demo memproses user ini — supaya 1 user
--   maksimal diproses 1x per hari walau server restart.
ALTER TABLE users
    ADD COLUMN demo_initial_capital NUMERIC(28, 8),
    ADD COLUMN demo_robot_last_run_date DATE;
