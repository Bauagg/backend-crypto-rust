# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------------------------
# Tahap 1: build binary release (lto, strip — lihat [profile.release] di Cargo.toml).
# ---------------------------------------------------------------------------------------------
FROM rust:1-bookworm AS builder
WORKDIR /app

# Cache dependency: build dulu dengan main kosong supaya crate pihak ketiga tidak di-compile ulang
# setiap kali kode di src/ berubah.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/backend_treding_rust* target/release/backend-treding-rust*

# Kode asli. `migrations/` ikut karena `sqlx::migrate!` menanamkannya ke binary saat compile.
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

# ---------------------------------------------------------------------------------------------
# Tahap 2: image runtime kecil — hanya binary + sertifikat CA (HTTPS ke Binance/CoinGecko).
# ---------------------------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 tzdata \
    && rm -rf /var/lib/apt/lists/*

# Jalan sebagai user biasa, bukan root.
RUN useradd --system --create-home --uid 10001 app
WORKDIR /app
RUN mkdir -p /app/files /app/logs && chown -R app:app /app

COPY --from=builder /app/target/release/backend-treding-rust /usr/local/bin/backend-treding-rust

USER app
ENV PORT=8000 \
    PATH_FILE_UPLOAD=files \
    RUST_LOG=info
EXPOSE 8000

CMD ["backend-treding-rust"]
