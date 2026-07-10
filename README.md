# backend-treding-rust

Backend infrastruktur inti (Rust + axum + sqlx), mengikuti pola dari `be-asmi-express`: JWT auth, response format standar, error handling terpusat, middleware, koneksi database, dan router index. Belum ada modul bisnis (users/products/dll) — ini adalah skeleton infrastrukturnya saja, siap dipakai untuk menambah modul baru.

## Struktur folder

```
src/
├── main.rs                    entry point: load env, init logger, connect DB, build router, listen
├── config/
│   └── logger.rs               init tracing (console + file logs/combined.log)
├── database/
│   └── mod.rs                  buat PgPool (postgres) + jalankan migration
├── middlewares/
│   └── authenticate.rs         verifikasi JWT Bearer token, inject claims ke request
├── router/
│   └── mod.rs                  index gabungan seluruh route service, di-nest di /api
└── utils/
    ├── api_response.rs         helper response format standar (success, created, paginated)
    ├── app_error.rs            enum AppError + mapping ke HTTP response + error sqlx
    ├── jwt.rs                  sign/verify access & refresh token
    └── bcrypt.rs               hash & verify password
```

Setiap folder punya `mod.rs` sebagai index yang mendeklarasikan file-file di dalamnya (setara `index.ts`).

## Requirement

- Rust (edisi 2024) & Cargo
- PostgreSQL yang sudah berjalan

## Setup

1. Copy `.env.example` menjadi `.env`, lalu sesuaikan `DATABASE_URL` / `DB_*`, `JWT_SECRET`, `JWT_REFRESH_SECRET`, dll.
2. Pastikan database yang disebut di `DATABASE_URL` sudah dibuat di PostgreSQL.
3. Kalau nanti ada migration SQL, taruh di folder `migrations/` — dijalankan otomatis saat start.

## Menjalankan project

```bash
cargo run
```

Untuk auto-reload saat development, install `cargo-watch` sekali:

```bash
cargo install cargo-watch
cargo watch -x run
```

Build rilis (binary optimized):

```bash
cargo build --release
./target/release/backend-treding-rust
```

Server berjalan di `http://localhost:<PORT>` (default `3000`, sesuai `.env`). Endpoint root:

```
GET / → { "message": "Server is running" }
```

## Menambah modul baru (mis. `users`, `products`)

Ikuti pola layered dari `be-asmi-express`: buat folder `src/services/<nama>/` berisi `model.rs`, `types.rs`, `repository.rs`, `service.rs`, `controller.rs`, `route.rs`, lalu:

1. Daftarkan folder itu di `src/services/mod.rs` (`pub mod <nama>;`) — buat file ini kalau belum ada.
2. Tambahkan `mod services;` di `main.rs`.
3. Nest router service tersebut di `src/router/mod.rs`.
4. Tambahkan migration SQL terkait di `migrations/`.

Endpoint yang butuh login tinggal pasang layer `axum::middleware::from_fn(middlewares::authenticate::authenticate)` pada route/group yang sesuai, lalu ambil identitas user via `Extension<JwtClaims>` di controller.
