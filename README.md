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
│   ├── coin_symbols/      worker: sinkron daftar coin dengan Binance + CoinGecko
│   ├── candle_ohlcv/      worker: kumpulkan candle harian (1d) ke DB
│   ├── fear_greed/        worker: kumpulkan Fear & Greed Index ke DB
│   └── market/            harga, chart (cache Redis), candle live (WebSocket hub), rekomendasi
├── database/            PgPool + migration otomatis, pool Redis
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
├── types.rs        struct request/response
└── worker.rs       job background (kalau ada), dijalankan dari main.rs
```

Pemanggilan API luar **tidak** ditulis di service — selalu lewat `src/clients/`.

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
   | `COINGECKO_API_URL`, `FNG_API_URL` | sumber market cap & Fear & Greed |

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

## Endpoint

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

Tes WebSocket di Postman: **New → WebSocket**, isi `ws://localhost:<PORT>/api/market/ws?symbol=BTCUSDT&interval=1m`, lalu **Connect**.

## Troubleshooting

| Gejala | Penyebab |
|---|---|
| `Gagal konek ke Redis` saat start | container Redis belum jalan → `docker start redis` |
| `Gagal bind port` | port sudah dipakai proses lain |
| `/market/recommendations` → "Layanan rekomendasi sedang tidak bisa dihubungi" | API Python belum jalan |
| Rekomendasi tidak berubah setelah respons Python diubah | masih cache Redis (maks 1 jam) — hapus key `recommendations:*` |
| Coin baru dari sync tidak masuk | akun System gagal dibuat saat start — cek log `Gagal menyiapkan akun System` (akun `system@system.local` dibuat otomatis, tidak perlu diisi di `.env`) |

## Catatan deploy

- Isi `BASE_URL` dengan domain **https** sebelum server pertama kali jalan — URL foto disimpan di DB.
- Folder upload (`PATH_FILE_UPLOAD`) jadikan volume Docker supaya file tidak hilang saat container dibuat ulang.
- Batasi memori Redis: `--maxmemory 256mb --maxmemory-policy allkeys-lru`.
- Pilih VPS di luar AS (Binance memblokir IP AS); Singapura paling aman.
- Build binary di luar VPS (laptop/CI) kalau RAM VPS kecil — compile Rust butuh 1–2 GB RAM.
