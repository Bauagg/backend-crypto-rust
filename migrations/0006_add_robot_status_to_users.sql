-- Status aktif robot trading per user: dipisah demo (akun simulasi, demo_balance) dan platform
-- (akun real, pakai api_key/api_secret exchange milik user). Default false -- robot tidak jalan
-- otomatis begitu akun dibuat, user harus eksplisit mengaktifkan.
ALTER TABLE users
    ADD COLUMN is_robot_demo_active BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN is_robot_platform_active BOOLEAN NOT NULL DEFAULT false;
