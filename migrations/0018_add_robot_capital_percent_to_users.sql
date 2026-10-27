-- Batas modal robot per mode akun, dalam persen (1–100), diatur user sendiri. Default 100 = seluruh
-- saldo boleh dipakai, sama dengan aturan bot Python apa adanya (Python yang menentukan porsi tiap
-- coin). Modal robot = persen × (kas + nilai coin milik robot); hanya modal itu yang dikirim ke
-- /signal (`modal` & `cash_usdt`), jadi Python membagi modal tersebut ke semua coin, sisa kas tidak
-- disentuh robot. Contoh: saldo demo 1000 USDT, 10% -> robot hanya memakai 100 USDT.
ALTER TABLE users
    ADD COLUMN demo_robot_capital_percent NUMERIC(5, 2) NOT NULL DEFAULT 100
        CHECK (demo_robot_capital_percent >= 1 AND demo_robot_capital_percent <= 100),
    ADD COLUMN platform_robot_capital_percent NUMERIC(5, 2) NOT NULL DEFAULT 100
        CHECK (platform_robot_capital_percent >= 1 AND platform_robot_capital_percent <= 100);
