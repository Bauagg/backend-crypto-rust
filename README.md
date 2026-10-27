# backend-treding-rust

Backend sistem manajemen aset crypto (Rust + axum + sqlx + Redis).

Pembagian peran dengan API strategi Python ([`backtes-crypto`](../backtes-crypto)):

| Backend ini (Rust) | API strategi (Python) |
|---|---|
| User, login, data akun | Membaca DB, menghitung sinyal & rekomendasi |
| **Mengisi DB** yang dibaca Python: `candle_ohlcv`, `fear_greed_index`, `flex_params` (daftar coin) | Tidak pernah menulis ke DB |
| Data market untuk aplikasi (harga, chart, candle live) | |
| Meneruskan rekomendasi Python ke aplikasi; nanti: eksekusi order ke Binance | Memutuskan coin apa yang dibeli/dijual |

Kontrak data dengan Python ada di `backtes-crypto/docs/API_RUST.md`.

## Struktur folder

```
src/
├── main.rs              entry point: env, logger, DB, Redis, worker, router
├── clients/             SATU-SATUNYA tempat yang memanggil API luar
│   ├── binance.rs         klines, exchangeInfo, ticker, stream WS, request bertanda tangan (akun)
│   ├── coingecko.rs       market cap & logo coin
│   ├── alternative_me.rs  Fear & Greed Index
│   └── strategy_api.rs    API strategi Python (rekomendasi)
├── services/            modul bisnis (lihat "Pola modul" di bawah)
│   ├── users/             register, login, profil, API key Binance
│   ├── documents/         upload file (disimpan di folder PATH_FILE_UPLOAD)
│   ├── flex_params/       parameter fleksibel, termasuk daftar coin (SIMBOL_CRYPTO)
│   ├── coin_symbols/      sinkron daftar coin dengan Binance + CoinGecko
│   ├── candle_ohlcv/      candle harian (1d) di DB
│   ├── fear_greed/        Fear & Greed Index di DB
│   ├── transactions/      posisi trading, BUY/SELL manual, kepemilikan coin, dashboard
│   ├── screeners/         strategi screening user (user_screeners) + menjalankan screening
│   ├── screener_rules/    indikator + kondisi di dalam strategi (user_screener_rules), skor per rule
│   ├── strategies/        strategi script buatan user (strategies) + validasi, hasil, chat AI penulis script
│   ├── strategy_versions/ isi script per versi (strategy_versions) — immutable
│   ├── strategy_subscriptions/ user memakai strategi (miliknya / hasil Store) sebagai SCREENER atau BOT
│   ├── strategy_backtests/ backtest strategi (statistik, ketahanan, kurva modal)
│   ├── strategy_backtest_trades/ daftar trade hasil backtest
│   ├── store_listings/    Store: strategi yang dipasang pembuatnya (tanpa isi script)
│   ├── store_promos/      kode promo dari pemilik listing
│   ├── store_purchases/   hak pakai strategi Store (beli / trial)
│   └── market/            daftar coin + harga, chart REST (cache Redis), rekomendasi
├── indicator/           rumus indikator teknikal, 1 file = 1 indikator (nama = value_param INDICATORS di flex_params)
├── strategy_engine/     runtime script strategi user (bahasa Rhai, sandbox) — fungsi = rumus indicator/;
│                          backtest.rs (simulasi), validation.rs (uji overfitting), insights.rs (analisa faktor)
├── robot/               mesin robot trading harian: demo.rs, live.rs (robot Python), strategy.rs (bot strategi user), common.rs (hitungan order bersama)
├── websocket/           SEMUA WebSocket real-time: hub chart (KlineHub) & harga live (TickerHub)
├── template/            template email yang dipakai ulang (mis. OtpEmail: kata-katanya lewat parameter)
├── worker/              SEMUA job background terdaftar di sini (lihat "Worker" di bawah)
├── database/            PgPool + migration otomatis, pool Redis, helper transaksi (begin/commit/Tx)
├── middlewares/         authenticate (JWT Bearer)
├── router/              gabungan route semua service di bawah /api
├── config/              logger (console + logs/)
└── utils/               response standar, AppError, JWT, bcrypt, enkripsi, HTTP client
```

### Pola modul

```
services/<modul>/
├── route.rs        daftar endpoint
├── controller.rs   terima request HTTP
├── service.rs      logika bisnis
├── repository.rs   akses data (Postgres, atau Redis seperti di market)
├── model.rs        struct tabel
└── types.rs        struct request/response
```

Pemanggilan API luar **tidak** ditulis di service — selalu lewat `src/clients/`.

### Aturan kode (wajib diikuti supaya tidak campur aduk)

**1. Tiap lapisan punya 1 tugas** — alur request: `route → controller → service → repository`.

| File | Boleh | Tidak boleh |
|---|---|---|
| `route.rs` | path + middleware | logika apa pun |
| `controller.rs` | ambil input HTTP (path, query, body JSON, multipart, token lewat `claims.user_uuid()`), panggil **1** fungsi service, bungkus respons (`success`/`created`/`paginated`) | membuka transaksi DB, query DB, panggil `clients/`, hitungan bisnis |
| `service.rs` | logika bisnis, validasi, **memegang transaksi DB** (`database::begin`/`commit`), panggil `clients/` | menulis SQL, membaca `axum` request |
| `repository.rs` | SQL saja, menerima `&mut Transaction` dari service | logika bisnis, panggil API luar |
| `model.rs` / `types.rs` | struct baris tabel / struct request & response | fungsi yang mengakses DB/API |

**2. Transaksi DB** — dibuka di service, sesingkat mungkin:
- Fungsi service untuk controller menerima `pool: &PgPool` dan dinamai `<aksi>_service`.
- Fungsi yang dipanggil service **modul lain** di dalam transaksi pemanggil menerima `tx: &mut Tx`
  dan dinamai `<aksi>_in_tx` (mis. `documents::upload_file_in_tx` dipakai upload foto profil).
- Jangan menahan transaksi saat menunggu hal lambat: panggilan Binance/API luar, hash bcrypt,
  kirim email — kerjakan sebelum `begin` atau setelah `commit`.
- Tidak perlu `rollback()` manual: transaksi yang tidak di-`commit` (keluar lewat `?`) otomatis
  di-rollback.

**3. Folder di luar `services/` — masing-masing 1 jenis tugas:**

| Folder | Isinya | Aturan |
|---|---|---|
| `clients/` | panggilan HTTP/WS ke API luar | tidak menyentuh DB |
| `worker/` | **jadwal** job background | cukup memanggil 1 fungsi (service/robot) + log hasil; logikanya tidak di sini |
| `robot/` | mesin robot harian | dipanggil worker `robot`; hitungan eksekusi order yang dipakai juga oleh BUY/SELL manual ada di `robot/common.rs` |
| `websocket/` | koneksi real-time ke FE + stream Binance | baca data lewat repository/service modul lain; tidak menulis DB |
| `indicator/` | rumus indikator (RSI, MACD, ...) | hitungan murni tanpa DB/API; 1 file per indikator, nama file = `value_param` INDICATORS di flex_params |
| `strategy_engine/` | runtime script strategi user (Rhai) | sandbox: hanya fungsi yang didaftarkan (`functions.rs`), tanpa file/jaringan/DB/import/eval, dibatasi jumlah operasi; dijalankan lewat `spawn_blocking` |
| `template/` | HTML/teks email | tanpa I/O — kata-kata dari parameter |
| `database/` | koneksi Postgres/Redis, helper transaksi | |
| `utils/` | helper umum tanpa pengetahuan fitur (response, error, JWT, enkripsi, mailer) | tidak boleh import dari `services/` |

**4. Fitur baru, taruh di mana?**
- Endpoint REST baru → `services/<modul>/` (pola di atas).
- Job terjadwal → logika di service/robot, jadwalnya `worker/<fitur>.rs` + daftarkan di `WORKERS`.
- Stream real-time → `websocket/`, daftarkan di `websocket::router()`.
- API pihak ketiga baru → `clients/<nama>.rs`.
- Indikator baru (mis. ZigZag) → `indicator/zigzag.rs` + test, daftarkan `mod` & varian
  `IndicatorKind` di `indicator/mod.rs`, lalu tambahkan baris `INDICATORS` di flex_params.

### Worker

Semua job background didaftarkan di satu tempat, `src/worker/mod.rs` (`WORKERS`), dan dijalankan
dengan cara yang sama: error/panic satu worker hanya dicatat (server & worker lain tetap jalan),
dan putaran berikutnya baru mulai setelah putaran sebelumnya selesai (tidak tumpang tindih).

| Worker | File | Jadwal |
|---|---|---|
| `coin-symbols` | `worker/coin_symbols.rs` | saat start (duluan, `prepare`) + tiap 24 jam |
| `candle-ohlcv` | `worker/candle_ohlcv.rs` | saat start + tiap xx:00:30 & xx:30:30 UTC |
| `fear-greed` | `worker/fear_greed.rs` | saat start + tiap 6 jam |
| `robot` | `worker/robot.rs` | tiap hari 00:45 UTC, coba ulang tiap 15 menit s/d 22:00 UTC |
| `strategy-bot` | `worker/strategy_bot.rs` | tiap hari 01:00 UTC, coba ulang tiap 15 menit s/d 22:00 UTC |

Worker cukup memanggil service/robot — logikanya tetap di `services/<modul>/service.rs` atau
`robot/`. Menambah worker:
buat `src/worker/<fitur>.rs` berisi `pub const WORKER`, lalu tambahkan ke `WORKERS`.

### WebSocket

Semua koneksi real-time ada di `src/websocket/`. `websocket/mod.rs` membuat hub, menjalankan stream
yang harus hidup terus, dan mendaftarkan endpoint; `main.rs` cukup memanggil `websocket::start`.

| Endpoint (FE) | Hub | Koneksi ke Binance |
|---|---|---|
| `WS /api/market/ws?symbol=BTCUSDT&interval=1h` (chart) | `kline_hub.rs` | 1 per simbol+interval yang sedang ditonton, ditutup otomatis kalau tidak ada yang menonton |
| `WS /api/market/ws/tickers?symbols=BTCUSDT,ETHUSDT` (daftar coin) | `ticker_hub.rs` | 1 untuk semua coin aktif, jalan terus sejak server start |

Di domain HTTPS pakai `wss://`, di lokal `ws://`.

## Requirement

- Rust & Cargo
- PostgreSQL
- Redis (paling mudah lewat Docker)
- API strategi Python di port 8080 — hanya untuk endpoint rekomendasi

## Setup

1. Copy `.env.example` menjadi `.env`, lalu isi:

   | Variabel | Isi |
   |---|---|
   | `DATABASE_URL` | koneksi PostgreSQL (database-nya harus sudah dibuat) |
   | `REDIS_URL` | koneksi Redis, mis. `redis://:password@127.0.0.1:6379` |
   | `JWT_SECRET`, `JWT_REFRESH_SECRET` | secret token login |
   | `ENCRYPTION_KEY` | tepat 32 karakter, untuk enkripsi API secret Binance user |
   | `BASE_URL` | URL publik backend (dipakai di URL file/foto). **Di server isi domain https sebelum pertama jalan** |
   | `PORT` | port server |
   | `MARKET_API_BASE_URL`, `MARKET_WS_BASE_URL` | Binance (atau Tokocrypto sebagai cadangan) |
   | `STRATEGY_API_BASE_URL` | API strategi Python, default `http://localhost:8080` |
   | `STRATEGY_API_KEY` | dikirim sebagai header `X-API-Key` ke API Python |
   | `AI_ROUTER_BASE_URL`, `AI_ROUTER_API_KEY`, `AI_ROUTER_MODEL` | 9router (format OpenAI, base URL termasuk `/v1`) untuk asisten AI screener & penulis script. Model = nama combo di 9router (mis. `AI-Crypro`) |
   | `COINGECKO_API_URL`, `FNG_API_URL` | sumber market cap & Fear & Greed |
   | `SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_FROM_NAME`, `SMTP_FROM_EMAIL` | email (OTP lupa password). Gmail: `SMTP_PASSWORD` = App Password 16 huruf, bukan password Gmail |

2. Jalankan Redis (sekali saja, selanjutnya ikut jalan otomatis bersama Docker):

   ```bash
   docker run -d --name redis -p 6379:6379 --restart unless-stopped redis redis-server --requirepass "<password>"
   ```

Migration di folder `migrations/` dijalankan otomatis saat server start.

## Menjalankan project

```bash
cargo run
```

Server siap saat muncul `Server running on http://localhost:<PORT>`. Untuk auto-reload saat development:

```bash
cargo install cargo-watch   # sekali saja
cargo watch -x run
```

Build rilis:

```bash
cargo build --release
./target/release/backend-treding-rust
```

Untuk endpoint rekomendasi, jalankan juga API Python di folder `backtes-crypto`:

```bash
.\venv\Scripts\python.exe main.py
```

### Worker yang ikut jalan otomatis

| Worker | Jadwal | Tugas |
|---|---|---|
| `coin_symbols` | saat start, lalu tiap 24 jam | samakan daftar coin `SIMBOL_CRYPTO` dengan Binance: crypto asli (top 250 CoinGecko, bukan saham/stablecoin/emas), volume 24 jam ≥ $5 juta (keluar kalau < $3 juta). Kategori Large/Mid/Small dari market cap, logo dari CoinGecko. Coin punya logo = aktif |
| `candle_ohlcv` | saat start, lalu tiap xx:00:30 & xx:30:30 UTC | isi candle harian semua coin (backfill 500 hari untuk coin baru), simbol yang sudah lengkap dilewati |
| `fear_greed` | saat start, lalu tiap 6 jam | isi Fear & Greed Index |
| `robot` | tiap hari 00:45 UTC | robot Python DEMO & LIVE (lihat [Robot Python](#2-robot-python-demo--live)) |
| `strategy-bot` | tiap hari 01:00 UTC | bot strategi script user (lihat [Bot strategi](#bot-strategi-user)) |

Robot & bot hanya jalan kalau candle harian kemarin sudah masuk DB; kalau belum, dicoba lagi tiap
15 menit sampai 22:00 UTC. Tiap robot/bot diproses maksimal 1x per hari.

## Penjelasan fitur

Semua fitur trading memakai **candle harian (D1)**: sinyal dihitung dari candle yang sudah close,
bukan harga per menit. Semua akun punya 2 mode: **DEMO** (saldo latihan `users.demo_balance`,
order disimulasikan) dan **LIVE** (order asli ke Binance pakai API key user).

### 1. BUY / SELL manual (`/transactions`)

Satu endpoint untuk beli & jual: `POST /transactions` dengan `transaction_type` `BUY` / `SELL`.

```json
{ "transaction_type": "BUY", "flex_param_id": "<id coin>", "account_mode": "DEMO",
  "amount": 30, "currency": "USDT" }
```

- `amount` = nilai yang dibeli/dijual dalam `currency` (`USDT` atau `IDR`; kosong =
  `preferred_currency` user). IDR dikonversi ke USDT dengan kurs Binance saat itu.
- **DEMO & LIVE ikut aturan Binance spot** (hitungan di `robot/common.rs`, dipakai juga robot):
  qty dibulatkan ke `stepSize`, order di bawah minimum notional (±5 USDT) ditolak, fee 0,1%
  (BUY dipotong dari coin, SELL dari USDT), harga = harga pasar saat itu.
- **Jual semua coin**: SELL dengan `"coin_all": true` (tanpa `amount`) — seluruh coin itu dijual
  berapa pun harganya saat order, jadi tidak tersisa walau harga bergerak sejak layar dibuka.
  `coin_all` ditolak untuk BUY.
- **SELL memakai FIFO**: posisi BUY tertua ditutup lebih dulu. Jual sebagian = posisi dipecah,
  bagian yang terjual dicatat dengan harga jual & untung/ruginya, sisanya tetap OPEN. Jual rugi
  tetap tercatat (profit negatif) — riwayat tidak pernah hilang.
- **Asal keputusan BUY** dicatat di `trade_positions.rule_type` + `rule_id` untuk analisa strategi
  mana yang paling profit:

  | `rule_type` | Arti | `rule_id` |
  |---|---|---|
  | `MANUAL` | beli sendiri (default) | kosong |
  | `SCREENER` | dari hasil screener (kirim `screener_id`) | id screener |
  | `RECOMMENDATION` | dari rekomendasi coin Python | kosong |
  | `STRATEGY` | dari strategi script (kirim `strategy_subscription_id`) / bot strategi | id versi strategi |
  | `ROBOT` | robot Python | kosong |

  Tidak ada CHECK di DB — jenis baru cukup ditambahkan di kode.
- `GET /transactions/holding` & `/holding/:symbol`: coin yang dipegang, harga rata-rata, nilai &
  untung/rugi pakai **harga terkini**, plus `available_balance` (saldo USDT yang menganggur).
- `GET /transactions/dashboard`: ringkasan nilai akun & performa.

### 2. Robot Python (DEMO & LIVE)

Robot bawaan platform. Tiap hari worker `robot` meminta sinyal ke API Python (`POST /signal`, isi
kontrak di `backtes-crypto/docs/API_RUST.md`) lalu menjalankan order-nya.

- Aktif per user lewat profil: `is_robot_demo_active` (DEMO) dan `is_robot_platform_active` (LIVE,
  wajib API key Binance).
- **Modal robot** = `demo_robot_capital_percent` / `platform_robot_capital_percent` % saldo
  (default 100% = semua saldo, sama dengan aturan Python). Sisa saldo tidak disentuh robot.
- Robot hanya mengurus coin yang dibelinya sendiri (`rule_type = ROBOT`). Coin yang dibeli manual
  tidak pernah dijual robot.
- **Recovery**: kalau user menjual sendiri coin milik robot, robot tidak langsung membeli ulang —
  `demo_robot_awaiting_recovery` / `platform_robot_awaiting_recovery` = true dan robot menunggu
  jadwal rebalance strategi berikutnya.
- LIVE: order MARKET asli. Order yang berhasil langsung dicatat di transaksinya sendiri (order
  exchange tidak bisa di-rollback); respons Binance disimpan sebagai bukti.

### 3. Screener (`/screeners`)

Screener sederhana tanpa coding: user memilih indikator + kondisi (1–8 rule, indikator =
`INDICATORS` di flex_params, rumusnya di `src/indicator/`).

- 1 user = 1 screener; `PUT /screeners` membuat atau mengganti seluruh isinya.
- `GET /screeners/results`: semua coin aktif dinilai 0–100 (rata-rata skor rule dibobot `weight`),
  7–20 coin teratas ditampilkan. Hasil tidak disimpan — dihitung setiap diminta.
- `POST /screeners/ai-chat`: chat dengan AI (9router) untuk menyusun screener. Balasannya `reply`
  + `draft` yang siap dikirim ke `PUT /screeners` (tombol "Simpan" di FE). AI hanya menjawab topik
  screener/indikator dan selalu mengingatkan bahwa datanya candle harian.

### 4. Strategi script (`/strategies`)

Seperti Pine Script di TradingView: user menulis script sendiri (bahasa **Rhai**, mirip
JavaScript) yang menilai tiap coin 0–100. Dokumentasi lengkap untuk editor FE:
`GET /strategies/reference`.

```rhai
let r = last(rsi(close, inputs.rsi_period));
let tren_naik = last(close) > last(ema(close, 200));
if tren_naik && r < 35 { signal(90, "RSI murah di tren naik", 25) }
else { signal(0, "belum cocok") }
```

- **Data untuk script**: `open`, `high`, `low`, `close`, `volume` (300 candle harian terakhir),
  `symbol`, `fear_greed`, `inputs`. Fungsi: 13 indikator (sma, ema, rsi, macd, bollinger,
  stoch_rsi, stochastic, atr, adx, vwap, obv, ichimoku, supertrend) + bantuan (last, prev,
  highest, lowest, change_percent, crossover, crossunder, is_na).
- **Hasil**: angka 0–100, `signal(skor, "alasan")`, atau `signal(skor, "alasan", persen)` — persen
  = ukuran posisi khusus coin itu saat dipakai sebagai bot.
- **Sandbox aman**: tanpa akses file/jaringan/DB, tanpa `import`/`eval`, maksimal 1.000.000
  operasi per coin (loop tanpa akhir dihentikan), maksimal 20.000 karakter. Script rusak hanya
  gagal sendiri, server tetap jalan.
- **Jenis** (`kind`): `SCREENER` (default) atau `BOT` — label kategori (tampilan & filter Store); pemakaiannya ditentukan `coin_scope`. **Status**: `DRAFT`, `ACTIVE`,
  `ARCHIVED`. Maksimal 20 strategi per user.
- **Versi immutable**: tiap simpan script = versi baru (`POST /strategies/:id/versions`), versi
  lama tidak pernah berubah. Pemakai tetap di versi lamanya sampai memilih `upgrade_to_latest`,
  jadi bot tidak berubah perilaku diam-diam.
- **Setelan (`inputs`)** yang boleh diubah pemakai, seperti `input()` di Pine Script:
  `[{ "name": "rsi_period", "label": "Periode RSI", "default": 14, "min": 2, "max": 50, "integer": true }]`.
- **Aturan bot per versi** (ditentukan pembuat):

  | Field | Default | Arti |
  |---|---|---|
  | `entry_score` | 70 | coin DIBELI kalau skor ≥ nilai ini |
  | `exit_score` | 30 | coin DIJUAL kalau skor ≤ nilai ini (harus < `entry_score`) |
  | `position_percent` | 20 | ukuran tiap posisi (% modal bot) kalau script tidak menentukan |
  | `max_positions` | 5 | maksimal coin dipegang bersamaan |
  | `coin_scope` | `ALL` | `ALL` = semua coin aktif (bisa juga dipakai sebagai screener), `SPECIFIC` = hanya coin di `symbols` (khusus bot) |

- `POST /strategies/validate`: coba script tanpa menyimpan (compile + jalankan ke semua coin).
- `POST /strategies/ai-chat`: AI menulis/mengubah script; drafnya sudah dicoba ke semua coin
  sebelum dikirim balik.
- **Isi script hanya bisa dilihat pemiliknya.** User lain hanya bisa memakai (lewat Store).
- **Detail strategi (`GET /strategies/:id`) berisi `stats`** — dihitung sistem, sama persis dengan
  yang tampil di Store:

  | Field | Isi |
  |---|---|
  | `official_backtest` | backtest resmi versi aktif: `stats` (return, max drawdown, win rate, profit factor, Sharpe, vs BTC, ...), `robustness` (KUAT/SEDANG/LEMAH), `analysis` (kelebihan/kekurangan). `null` = versi aktif belum dibacktest pemilik |
  | `equity_curve` | kurva modal & drawdown harian backtest resmi (untuk grafik) |
  | `live_performance` | performa transaksi asli **semua pemakai** (semua versi), 1 baris per `account_mode` — DEMO & LIVE dipisah: jumlah trade tutup, win rate, profit terealisasi (USDT & %), posisi terbuka, jumlah user |
  | `my_performance` | sama, tapi hanya transaksi **user yang melihat** |

  Transaksi yang dihitung = posisi `rule_type = STRATEGY` dari versi strategi ini (bot strategi +
  BUY manual dari hasil strategi).

### 5. Backtest (`/strategy-backtests`)

Simulasi bot di histori **365 hari**, aturan sama persis dengan bot sungguhan:

- sinyal dari candle **close hari D**, order di harga **open hari D+1** (tidak pernah melihat masa
  depan);
- jual coin skor ≤ `exit_score`, lalu beli coin skor ≥ `entry_score` (skor tertinggi dulu) selama
  posisi < `max_positions`;
- **modal awal sesuai input user** (`initial_capital` 10–1.000.000 USDT, default 1.000), fee 0,1%,
  order < 5 USDT dilewati (minimum Binance) — jadi modal kecil bisa memberi hasil berbeda;
- pembanding: beli-tahan BTC di periode yang sama.

Skor script dihitung sekali per coin per hari, lalu simulasi order bisa diulang murah — dipakai uji
sensitivitas. Kode: `strategy_engine/backtest.rs` (simulasi), `validation.rs` (overfitting),
`insights.rs` (analisa faktor). Satu backtest lengkap 46 coin × 365 hari ±1,5 detik (build release).

Hasil yang disimpan:

| Bagian | Isi |
|---|---|
| `stats` | total return, CAGR, **max drawdown**, Sharpe, Sortino, volatilitas, jumlah trade, win rate, profit factor, rata-rata untung/rugi, trade terbaik/terburuk, lama pegang, exposure, return vs BTC, return per bulan |
| `robustness` | label **KUAT / SEDANG / LEMAH**, return paruh awal vs akhir, % bulan profit, drawdown terlama (hari), rugi beruntun terpanjang, `overfitting_risk` |
| `analysis` | kelebihan & kekurangan dalam bahasa sederhana (termasuk hasil uji overfitting) |
| `validation` | **uji overfitting** (lihat di bawah) + skor risiko 0–100 |
| `insights` | **faktor yang paling memengaruhi untung/rugi** + saran (lihat di bawah) |
| `equity_curve` | kurva modal harian + drawdown + kurva BTC (untuk grafik) |
| trades | `GET /strategy-backtests/:id/trades` |

Label ketahanan:
- **LEMAH** — rugi, atau drawdown lebih dalam dari −50%, atau kurang dari 5 trade.
- **KUAT** — untung di paruh awal & akhir, minimal 20 trade, drawdown lebih kecil dari −30%,
  profit factor ≥ 1,3, dan risiko overfitting tidak TINGGI.
- **SEDANG** — selain itu.

**Uji overfitting (`validation`)** — apakah hasil bagus itu nyata atau kebetulan / terlalu pas
dengan data masa lalu. Acak memakai seed tetap: backtest yang sama selalu memberi hasil sama.

| Uji | Cara | Tanda overfitting |
|---|---|---|
| `out_of_sample` | 70% awal ("latihan") vs 30% akhir ("ujian"); `consistency_ratio` = return/tahun ujian ÷ latihan | ratio < 0,3 |
| `monte_carlo` | return harian diambil ulang acak per blok 10 hari, 1.000 skenario → return & drawdown persentil 5/50/95 + `probability_of_loss_percent` | peluang rugi ≥ 30% |
| `random_baseline` | tiap trade diganti beli coin & tanggal acak dengan lama pegang sama, 300x → `p_value` | p-value > 0,2 (tidak lebih baik dari acak) |
| `sensitivity` | `entry_score` ±5/±10, `exit_score` ±10, 2 setelan script pertama ±20% → return tiap variasi | < 50% variasi profit |

`overfitting_risk`: skor 0–100 (tiap uji maks 25; trade selesai < 20 menambah 15) → **RENDAH**
(< 30) / **SEDANG** (30–59) / **TINGGI** (≥ 60). `TIDAK_RELEVAN` = strategi tidak profit (bukan
overfit, memang belum bagus); `DATA_KURANG` = tidak ada uji yang bisa dijalankan.

**Analisa faktor (`insights`)** — seperti *feature importance* di ML. Trade dikelompokkan per
faktor: coin, kondisi pasar (BTC 30 hari naik/sideways/turun), Fear & Greed saat masuk, skor
masuk, lama pegang, volatilitas coin (ATR 14 hari), hari masuk. Per faktor:
- `importance_percent` — epsilon-squared: porsi beda untung/rugi antar trade yang dijelaskan faktor
  (dikoreksi jumlah kelompok; hanya kelompok ≥ 3 trade);
- `p_value` + `significant` — uji permutasi 500x: apakah pengaruh itu lebih besar dari kebetulan
  (p < 0,1);
- `groups` — per kelompok: jumlah trade, win rate, rata-rata return, total profit;
- `finding` — kelompok terbaik & terburuk.

`recommendations` hanya diambil dari faktor yang **signifikan** (mis. "naikkan entry_score ke 80",
"coin yang paling sering rugi: …", "tambahkan syarat fear_greed < 75"). Butuh minimal 10 trade.

**Backtest resmi** (`is_official`, dipakai Store) = dijalankan **pemilik** dengan **modal 1.000
USDT & setelan default** — supaya semua strategi di Store dibandingkan dengan syarat yang sama.
Backtest pemilik dengan modal/setelan lain hanya terlihat oleh pemiliknya. Pemakai lain boleh
backtest dengan modal & setelannya sendiri. Maksimal 2 backtest jalan bersamaan di server.

### 6. Memakai strategi: screener atau bot (`/strategy-subscriptions`)

User memakai strategi miliknya sendiri, atau strategi Store yang sudah dibeli/trial.

```json
{ "strategy_id": "<id>", "usage": "BOT", "account_mode": "DEMO", "capital_percent": 30,
  "input_values": { "rsi_period": 14 } }
```

- `usage = SCREENER`: cari coin skor tertinggi (`GET /:id/results`). Hanya untuk strategi
  `coin_scope = ALL`.
- `usage = BOT`: robot order otomatis. Wajib `account_mode` (`DEMO` / `LIVE`) dan
  `capital_percent` (1–100). LIVE wajib API key Binance. Boleh DEMO dan LIVE sekaligus
  (= 2 pemakaian).
- Bisa diubah: `capital_percent`, `input_values`, aktif/nonaktif (`is_active`), `upgrade_to_latest`.
- `GET /:id/performance`: performa asli bot (posisi terbuka/tertutup, profit, win rate).

### Bot strategi user

Dijalankan worker `strategy-bot` (`robot/strategy.rs`) tiap hari untuk semua pemakaian BOT yang
aktif:

1. Script versi yang dipakai dijalankan ke coin cakupannya + coin yang sedang dipegang bot.
2. **JUAL** seluruh coin yang skornya ≤ `exit_score` (atau sudah keluar dari cakupan).
3. **BELI** coin skor ≥ `entry_score` yang belum dipegang, skor tertinggi dulu, selama posisi <
   `max_positions`. Ukuran = persen dari script atau `position_percent` × modal bot, tidak lebih
   dari kas bot, dan wajib lolos aturan Binance.
4. **Modal bot** = `capital_percent`% × (kas + nilai coin bot). Sisa saldo user tidak disentuh.

- Tiap bot hanya mengurus posisinya sendiri (`trade_positions.strategy_subscription_id`,
  `rule_type = STRATEGY`) — tidak bercampur dengan robot Python atau beli manual.
- DEMO: semua order 1 bot dalam 1 transaksi DB (gagal = batal semua).
- LIVE: tanggal proses dicatat **sebelum** order dikirim, jadi order uang asli maksimal 1x per
  hari walau server restart.
- Bot dilewati kalau strategi diarsipkan/dihapus atau hak pakai Store sudah habis.

### 7. Store (`/store/...`)

Tempat pembuat strategi memasang strategi/bot-nya untuk dipakai user lain.

**Penjual**
1. Buat strategi, ubah status ke `ACTIVE`, jalankan backtest versi aktif.
2. `POST /store/listings` — pasang dengan harga & trial:

   | Field | Isi |
   |---|---|
   | `pricing_model` | `FREE` (default), `ONE_TIME` (berlaku selamanya), `MONTHLY` (30 hari) |
   | `price_idr` | harga Rupiah, wajib > 0 untuk ONE_TIME/MONTHLY |
   | `trial_days` | 0–90 hari trial gratis (0 = tanpa trial) |

3. `POST /store/promos` — kode promo: potongan 1–100%, masa berlaku (`starts_at` / `ends_at`),
   batas pemakaian (`max_redemptions`). Kode unik per listing, tidak peduli huruf besar/kecil.
4. `PUT /store/listings/:id` ubah harga/trial, `status = INACTIVE` untuk menyembunyikan;
   `DELETE` = cabut dari Store. Pembeli yang hak pakainya masih berlaku tetap bisa memakai sampai
   habis. Selama terpasang, strateginya tidak bisa dihapus.

**Yang dilihat pembeli** (`GET /store/listings`, `/store/listings/:id`)
- Halaman Store hanya berisi produk **user lain** — dagangan sendiri ada di `/store/listings/mine`.
  Pencarian `search` mencocokkan judul listing, nama strategi, atau nama pembuat.
- **Isi script tidak pernah dikirim** — supaya tidak bisa disalin. Yang tampil: judul, deskripsi,
  jenis, versi, aturan bot (skor masuk/keluar, ukuran posisi, cakupan coin).
- **Pembuat tampil dengan nama asli & fotonya.**
- **Statistik dihitung sistem, bukan diisi pembuat** — field `stats`, sama dengan detail strategi:
  backtest resmi versi aktif, kurva modal & drawdown (hanya di detail), performa transaksi asli
  semua pemakai & milik sendiri (DEMO & LIVE dipisah). Ditambah `active_users`.
- **Calon pembeli boleh melihat semuanya sebelum membeli**: statistik di atas, plus detail &
  daftar trade backtest resmi lewat `GET /strategy-backtests/<stats.official_backtest.id>` dan
  `/trades` — selama listing ACTIVE. Yang tidak bisa sebelum membeli: menjalankan strategi
  (hasil skor, screener, bot, backtest sendiri). Isi script tetap tidak pernah terlihat.
- `my_access`: hak pakai user (`OWNER` / `PURCHASE` / `TRIAL` + tanggal habis);
  `trial_available`: masih boleh trial.

**Checkout** (`POST /store/purchases`, body `{ "listing_id", "promo_code"?, "trial"? }`)
- `POST /store/purchases/quote` dulu untuk melihat rincian harga (harga, potongan, harga akhir).
- **Pembayaran masih dummy**: harga & potongan dicatat apa adanya, tapi yang dibayar **Rp0**
  (`payment_status = DUMMY`, atau `FREE` kalau harganya memang Rp0). Hak pakai langsung aktif.
  Saat payment gateway dipasang, cukup ganti langkah bayar di `checkout_service`.
- Aturan:
  - pemilik tidak bisa membeli strateginya sendiri (langsung pakai saja);
  - FREE / ONE_TIME tidak bisa dibeli dua kali;
  - MONTHLY dibeli lagi saat masih aktif = diperpanjang 30 hari dari tanggal habisnya;
  - trial sekali per user per listing, hanya kalau belum punya hak pakai; promo tidak berlaku
    untuk trial;
  - kuota promo dikunci saat checkout, jadi tidak terlampaui oleh checkout bersamaan.
- Setelah punya hak pakai, pakai strateginya lewat `POST /strategy-subscriptions` (screener/bot).
- `GET /store/purchases`: riwayat pembelian & trial.

### Alur lengkap (contoh)

```
Pembuat:  POST /strategies  →  PUT /strategies/:id {status: ACTIVE}  →  POST /strategy-backtests
          →  POST /store/listings  →  POST /store/promos
Pembeli:  GET /store/listings  →  GET /store/listings/:id  →  POST /store/purchases/quote
          →  POST /store/purchases  →  POST /strategy-subscriptions {usage: BOT, DEMO, 30%}
          →  (worker strategy-bot jalan tiap hari)  →  GET /strategy-subscriptions/:id/performance
```

## Endpoint

Dokumentasi untuk FE mobile (alur + curl) fitur strategi, AI, backtest, bot, Store, promo & trial:
[`docs/API_MOBILE_STRATEGY.md`](docs/API_MOBILE_STRATEGY.md).

Base URL: `http://localhost:<PORT>/api`. Format respons sukses:
`{ "status": "success", "message": "...", "data": ..., "meta"?: {...} }`; gagal: `{ "status": "fail" | "error", "message": "..." }`.

**Publik**

| Method | Path | Keterangan |
|---|---|---|
| POST | `/users/register` | daftar user |
| POST | `/users/login` | login, dapat access & refresh token |
| POST | `/users/refresh-token` | perbarui access token |
| GET | `/market/symbols?page=&limit=&search=` | daftar coin aktif + harga & perubahan 24 jam |
| GET | `/market/klines?symbol=&interval=&limit=&end_time=` | data chart (15 interval). Histori di-cache Redis per rentang waktu |
| WS | `/market/ws?symbol=&interval=` | candle live (1 koneksi ke Binance per simbol+interval, dibagi ke semua client) |
| GET | `/market/recommendations?limit=5..10&date=YYYY-MM-DD` | rekomendasi coin dari API Python (di-cache maks 1 jam) |
| GET | `/candle-ohlcv?symbol=&limit=&end_time=` | candle harian tersimpan di DB |
| GET | `/fear-greed?start=YYYY-MM-DD&end=YYYY-MM-DD` | histori Fear & Greed (default 90 hari) |
| GET | `/flex-params`, `/flex-params/:id`, `/flex-params/type/:type_param`, `/flex-params/header/:header_id` | baca flex params |

**Butuh login** (header `Authorization: Bearer <access_token>`)

| Method | Path | Keterangan |
|---|---|---|
| GET / PUT | `/users/profile` | lihat / ubah profil (termasuk API key Binance) |
| POST | `/flex-params` | buat (multipart, field `photo` opsional) |
| PUT / DELETE | `/flex-params/:id` | ubah / hapus (soft delete) |
| POST | `/files/upload` | upload file |
| GET / PUT / DELETE | `/files/:id` | lihat / ganti / hapus file |
| GET / PUT / DELETE | `/screeners` | strategi screening user — 1 user = 1 strategi; PUT membuat atau mengganti seluruh isinya (1–8 indikator) |
| GET | `/screeners/results` | jalankan screening: 7–20 coin skor tertinggi (0–100) dari candle harian, tidak disimpan |
| POST | `/screeners/ai-chat` | chat dengan asisten AI (9router) untuk menyusun / mengubah strategi; balik `reply` + `draft` siap dikirim ke PUT `/screeners`, tidak menyimpan apa pun |
| GET / POST | `/transactions` | riwayat posisi (search, filter, sort, paginasi) / BUY & SELL manual (`transaction_type`; SELL `coin_all: true` = jual semua) |
| GET | `/transactions/:id` | detail 1 posisi |
| GET | `/transactions/dashboard?account_mode=&source=&date_from=&date_to=` | ringkasan akun: modal, untung/rugi, kurva modal, per bulan, per coin, alokasi |
| GET | `/transactions/holding?flex_param_id=&account_mode=` / `/transactions/holding/:symbol?account_mode=` | coin yang dipegang + nilai sekarang + `available_balance` |
| GET / POST | `/strategies` | strategi script milik user / buat strategi + versi pertama (maks 20) |
| GET | `/strategies/reference` | dokumentasi API script (variabel, fungsi, format hasil) untuk editor FE |
| POST | `/strategies/validate` | coba script tanpa menyimpan: compile + jalankan ke semua coin |
| POST | `/strategies/ai-chat` | asisten AI menulis / mengubah script (draf sudah dicoba ke semua coin) |
| GET / PUT / DELETE | `/strategies/:id` | detail + `stats` (backtest resmi, kurva modal, performa transaksi DEMO & LIVE) — isi script hanya untuk pemilik / ubah nama-status / hapus (tidak bisa selama dipasang di Store) |
| GET / POST | `/strategies/:id/versions` | riwayat versi / simpan versi baru (versi lama tidak pernah berubah) |
| GET | `/strategies/:id/results?limit=` | jalankan versi aktif ke semua coin (skor 0–100), tidak disimpan |
| GET / POST | `/strategy-subscriptions` | strategi yang dipakai user / pakai strategi (milik sendiri atau hasil beli/trial Store) sebagai SCREENER, atau BOT (`account_mode` DEMO/LIVE + `capital_percent` 1–100) |
| PUT / DELETE | `/strategy-subscriptions/:id` | ubah setelan / `capital_percent` / aktif-nonaktif / `upgrade_to_latest` / berhenti memakai |
| GET | `/strategy-subscriptions/:id/results?limit=` | jalankan strategi yang dipakai (versi & setelan terkunci) |
| GET | `/strategy-subscriptions/:id/performance` | performa asli bot (posisi terbuka/tertutup, profit, win rate) |
| GET / POST | `/strategy-backtests?strategy_id=` | riwayat backtest / jalankan backtest 365 hari dengan modal pilihan (`initial_capital`): statistik, robustness, uji overfitting (`validation`), faktor berpengaruh (`insights`) |
| GET | `/strategy-backtests/:id` | detail backtest + kurva modal & drawdown harian |
| GET | `/strategy-backtests/:id/trades` | daftar trade hasil backtest |
| GET / POST | `/store/listings` | halaman Store — produk milik user lain saja (filter `kind`, `pricing_model`, `search` judul/nama strategi/nama pembuat, paginasi) / pasang strategi (wajib ACTIVE + sudah backtest) |
| GET | `/store/listings/mine` | listing milik penjual |
| GET / PUT / DELETE | `/store/listings/:id` | detail (statistik, kurva, pembuat, tanpa script) / ubah harga-trial-status / cabut dari Store |
| GET / POST | `/store/promos?listing_id=` | promo listing (khusus pemilik) / buat kode promo |
| PUT / DELETE | `/store/promos/:id` | ubah / hapus promo |
| POST | `/store/purchases/quote` | rincian harga dengan promo / trial tanpa membeli |
| GET / POST | `/store/purchases` | riwayat pembelian & trial / checkout (pembayaran masih dummy Rp0) |

Tes WebSocket di Postman: **New → WebSocket**, isi `ws://localhost:<PORT>/api/market/ws?symbol=BTCUSDT&interval=1m`, lalu **Connect**.

## Troubleshooting

| Gejala | Penyebab |
|---|---|
| `Gagal konek ke Redis` saat start | container Redis belum jalan → `docker start redis` |
| `Gagal bind port` | port sudah dipakai proses lain |
| `/market/recommendations` → "Layanan rekomendasi sedang tidak bisa dihubungi" | API Python belum jalan |
| Rekomendasi tidak berubah setelah respons Python diubah | masih cache Redis (maks 1 jam) — hapus key `recommendations:*` |
| Coin baru dari sync tidak masuk | akun System gagal dibuat saat start — cek log `Gagal menyiapkan akun System` (akun `system@system.local` dibuat otomatis, tidak perlu diisi di `.env`) |
| `Gagal konek ke database: PoolTimedOut` | server DB tidak terjangkau (jaringan / Postgres mati / firewall) — cek `Test-NetConnection <host> -Port <port>` |
| `Gagal menjalankan migration: VersionMismatch(N)` | file migration N diubah setelah masuk DB (checksum beda) — kembalikan isinya persis, perubahan baru selalu lewat migration baru. Bisa juga karena line ending CRLF (`.gitattributes` memaksa LF) |
| Error merah di VS Code tapi `cargo check` lolos | diagnostik rust-analyzer basi — simpan ulang file atau **rust-analyzer: Restart server** |

## Catatan deploy

- Isi `BASE_URL` dengan domain **https** sebelum server pertama kali jalan — URL foto disimpan di DB.
- Folder upload (`PATH_FILE_UPLOAD`) jadikan volume Docker supaya file tidak hilang saat container dibuat ulang.
- Batasi memori Redis: `--maxmemory 256mb --maxmemory-policy allkeys-lru`.
- Pilih VPS di luar AS (Binance memblokir IP AS); Singapura paling aman.
- Build binary di luar VPS (laptop/CI) kalau RAM VPS kecil — compile Rust butuh 1–2 GB RAM.
