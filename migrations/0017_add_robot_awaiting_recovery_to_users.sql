-- Robot "menunggu recovery" per mode akun. Diisi true saat user menjual MANUAL coin milik robot
-- (posisi source = BOT). Selama true, rebalance dari /signal yang alasannya HANYA "posisi tidak
-- sesuai target" (akibat coin robot dijual user) dilewati — robot tidak membeli ulang / menambah
-- coin sampai ada rebalance terjadwal strategi (rebalance bulanan tanggal 1, filter pasar berubah,
-- atau kill switch). Saat rebalance terjadwal itu dijalankan, kembali false. Juga dikosongkan saat
-- user mengaktifkan ulang robot.
ALTER TABLE users
    ADD COLUMN demo_robot_awaiting_recovery BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN platform_robot_awaiting_recovery BOOLEAN NOT NULL DEFAULT false;
