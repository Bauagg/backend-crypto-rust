# API Mobile — Strategi, AI, Backtest, Bot & Store

Dokumentasi untuk aplikasi mobile: membuat strategi (manual atau dengan AI), backtest, memakai
strategi sebagai screener/bot, dan Store (jual-beli strategi, promo, trial).

## Dasar

- **Base URL**: `https://<domain>/api` (lokal: `http://localhost:<PORT>/api`)
- **Semua endpoint di dokumen ini butuh login**: header `Authorization: Bearer <access_token>`.
  Token habis → `401`, minta token baru lewat `POST /users/refresh-token` `{ "refresh_token": "..." }`.
- Body JSON: header `Content-Type: application/json`.
- Data strategi selalu **candle harian (D1)** — sinyal dihitung dari candle yang sudah close, cocok
  untuk swing trading harian, bukan scalping. Tampilkan `timeframe_note` dari respons ke user.

Contoh curl memakai variabel shell:

```bash
BASE=http://localhost:8000/api
TOKEN=<access_token>
```

### Format respons

```json
// sukses
{ "status": "success", "message": "...", "data": { ... } }

// sukses + paginasi
{ "status": "success", "message": "...", "data": [ ... ],
  "meta": { "page": 1, "limit": 20, "total": 57, "total_pages": 3 } }

// gagal — tampilkan `message` ke user
{ "status": "fail", "message": "Jalankan backtest versi aktif dulu — ..." }

// gagal validasi field
{ "status": "fail", "message": "Validasi gagal",
  "errors": [ { "field": "name", "message": "Nama strategi wajib diisi (maksimal 100 karakter)" } ] }
```

| HTTP | Arti |
|---|---|
| 200 / 201 | sukses / berhasil dibuat |
| 400 | input salah (pesan di `message`) |
| 401 | belum login / token habis |
| 403 | tidak punya hak (mis. strategi Store belum dibeli) |
| 404 | tidak ditemukan (atau bukan milikmu) |
| 409 | bentrok (mis. sudah membeli, trial sudah dipakai, kode promo sudah ada) |
| 500 | error server |

### Istilah

| Istilah | Arti |
|---|---|
| **Strategi** | script buatan user yang menilai tiap coin dengan skor 0–100 |
| **Versi** | isi script. Tiap simpan = versi baru, versi lama tidak pernah berubah |
| **Skor** | 0–100 per coin. 100 = paling cocok dibeli |
| **Backtest** | simulasi bot di histori 365 hari |
| **Subscription / pemakaian** | user memakai strategi sebagai SCREENER atau BOT |
| **Listing** | strategi yang dipasang di Store |
| **Purchase** | hak pakai strategi Store (beli / trial) |

---

## Peta layar & endpoint

| Layar mobile | Endpoint |
|---|---|
| Strategi Saya (buatan sendiri) | `GET /strategies` |
| Buat strategi dengan AI | `POST /strategies/ai-chat` → `POST /strategies` |
| Editor script | `GET /strategies/reference`, `POST /strategies/validate`, `POST /strategies` / `POST /strategies/:id/versions` |
| Detail strategi | `GET /strategies/:id` (+ statistik), `GET /strategies/:id/results` |
| Backtest | `POST /strategy-backtests`, `GET /strategy-backtests?strategy_id=`, `GET /strategy-backtests/:id`, `/:id/trades` |
| Strategi yang Dipakai (screener/bot) | `GET /strategy-subscriptions`, `POST`, `PUT`, `DELETE`, `/:id/results`, `/:id/performance` |
| Store (semua produk) | `GET /store/listings`, `GET /store/listings/:id` |
| Produk Saya (yang dijual) | `GET /store/listings/mine`, `POST /store/listings`, `PUT` / `DELETE /store/listings/:id` |
| Promo produk | `GET /store/promos?listing_id=`, `POST`, `PUT`, `DELETE /store/promos/:id` |
| Checkout | `POST /store/purchases/quote` → `POST /store/purchases` |
| Produk yang Dibeli | `GET /store/purchases` |

---

## Alur 1 — Membuat strategi dengan AI

```
[Chat AI] --(draft)--> [Preview draf + tombol Simpan] --> POST /strategies --> [Detail strategi]
```

### 1.0 Jenis & cakupan coin

**`kind`** — label kategori untuk tampilan & filter Store: `SCREENER` (default) atau `BOT`.
Tidak membatasi pemakaian.

**`coin_scope`** — yang menentukan strategi bisa dipakai untuk apa:

| | `ALL` (default) — semua coin aktif | `SPECIFIC` — coin tertentu, mis. BTC saja |
|---|---|---|
| Isi `symbols` | kosong | 1–20 coin, boleh `"BTC"` atau `"BTCUSDT"` (otomatis jadi `BTCUSDT`) |
| Dipakai sebagai **BOT** | ✅ | ✅ hanya beli/jual coin di `symbols` |
| Dipakai sebagai **SCREENER** | ✅ cari coin skor tertinggi | ❌ ditolak (`400`) |
| Skor hari ini & backtest | semua coin | hanya coin di `symbols` |

Contoh strategi khusus BTC: `"coin_scope": "SPECIFIC", "symbols": ["BTC"]`. Di Store, cakupan ini
tampil di `strategy.bot.coin_scope` & `strategy.bot.symbols` — pembeli tahu sebelum membeli.

**Aturan bot** per versi (diisi pembuat; kosong = default / sama dengan versi sebelumnya):

| Field | Default | Arti |
|---|---|---|
| `entry_score` | 70 | coin DIBELI kalau skor ≥ nilai ini |
| `exit_score` | 30 | coin DIJUAL kalau skor ≤ nilai ini (harus < `entry_score`) |
| `position_percent` | 20 | % modal bot per coin kalau script tidak menentukan sendiri |
| `max_positions` | 5 | maksimal coin dipegang bersamaan (1–50) |

### 1.1 Chat dengan AI — `POST /strategies/ai-chat`

Kirim **seluruh riwayat chat** setiap kali (server tidak menyimpan chat). `role`: `user` atau
`assistant`. Isi `strategy_id` kalau user ingin AI **mengubah** strategi miliknya yang sudah ada.

```bash
curl -X POST $BASE/strategies/ai-chat -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "messages": [
    { "role": "user", "content": "Buatkan strategi beli saat RSI murah tapi tren masih naik di atas EMA 200" }
  ]
}'
```

Respons (bisa sampai ±60 detik — tampilkan loading, timeout HTTP minimal 120 detik):

```json
{
  "status": "success",
  "data": {
    "reply": "Strategi ini membeli coin saat RSI di bawah 35 dan harga di atas EMA 200 ...",
    "draft": {
      "name": "RSI Murah Tren Naik",
      "description": "...",
      "kind": "BOT",
      "source_code": "let r = last(rsi(close, inputs.rsi_period));\n...",
      "inputs": [ { "name": "rsi_period", "label": "Periode RSI", "default": 14, "min": 2, "max": 50, "integer": true } ],
      "change_note": null,
      "entry_score": 70, "exit_score": 30, "position_percent": 20, "max_positions": 5,
      "coin_scope": "ALL", "symbols": [],
      "preview": [ { "rank": 1, "symbol": "SOLUSDT", "photo_url": "...", "close": 142.3, "score": 90, "reason": "RSI murah di tren naik" } ]
    },
    "draft_errors": [],
    "timeframe_note": "Strategi dijalankan pada data candle harian ..."
  }
}
```

- `draft = null` → AI masih bertanya/menjelaskan. Tampilkan `reply` saja, lanjutkan chat.
- `draft` ada → tampilkan kartu draf + `preview` (5 coin teratas) + tombol **Simpan**. Draf sudah
  dicoba ke semua coin, jadi pasti jalan.
- `draft_errors` tidak kosong → AI membuat script yang salah. Tampilkan pesannya dan minta user
  melanjutkan chat (mis. "perbaiki").
- AI hanya menjawab topik strategi/indikator.

Lanjut chat (kirim ulang riwayat + balasan AI sebelumnya):

```bash
curl -X POST $BASE/strategies/ai-chat -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "messages": [
    { "role": "user", "content": "Buatkan strategi beli saat RSI murah tapi tren masih naik di atas EMA 200" },
    { "role": "assistant", "content": "Strategi ini membeli coin saat RSI di bawah 35 ..." },
    { "role": "user", "content": "Khusus BTC, ETH, SOL saja dan maksimal 3 coin" }
  ]
}'
```

### 1.2 Simpan draf — `POST /strategies`

Kirim isi `draft` apa adanya (tanpa `preview`), plus `ai_conversation` = riwayat chat (opsional,
untuk analisa).

```bash
curl -X POST $BASE/strategies -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "name": "RSI Murah Tren Naik",
  "description": "Beli saat RSI murah di tren naik",
  "kind": "BOT",
  "source_code": "let r = last(rsi(close, inputs.rsi_period));\nlet tren_naik = last(close) > last(ema(close, 200));\nif tren_naik && r < 35 { signal(90, \"RSI murah di tren naik\") } else { signal(0, \"belum cocok\") }",
  "inputs": [ { "name": "rsi_period", "label": "Periode RSI", "default": 14, "min": 2, "max": 50, "integer": true } ],
  "entry_score": 70,
  "exit_score": 30,
  "position_percent": 20,
  "max_positions": 3,
  "coin_scope": "SPECIFIC",
  "symbols": ["BTC", "ETH", "SOL"],
  "ai_conversation": [ { "role": "user", "content": "..." }, { "role": "assistant", "content": "..." } ]
}'
```

Respons `201` = detail strategi (lihat [1.5](#15-detail-strategi--get-strategiesid)). Strategi baru
berstatus `DRAFT`. Maksimal 20 strategi per user; nama unik per user.

### 1.3 Menulis script manual (editor)

**Dokumentasi script** untuk layar bantuan editor — `GET /strategies/reference`:

```bash
curl $BASE/strategies/reference -H "Authorization: Bearer $TOKEN"
```

Berisi teks referensi (variabel, 13 indikator, fungsi bantuan, contoh). Ringkasan:

```rhai
// variabel: open, high, low, close, volume (array, lama -> baru), symbol, fear_greed, inputs
let r = last(rsi(close, inputs.rsi_period));
if r < 30 { signal(90, "RSI oversold", 25) }   // skor, alasan, (opsional) % modal bot untuk coin ini
else { signal(0, "belum cocok") }
```

**Coba tanpa menyimpan** — `POST /strategies/validate` (tombol "Coba"):

```bash
curl -X POST $BASE/strategies/validate -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "source_code": "let r = last(rsi(close, inputs.rsi_period)); if r < 30 { signal(90, \"oversold\") } else { 0 }",
  "inputs": [ { "name": "rsi_period", "label": "Periode RSI", "default": 14, "min": 2, "max": 50, "integer": true } ],
  "input_values": { "rsi_period": 10 },
  "coin_scope": "ALL"
}'
```

```json
{ "data": {
    "valid": true,
    "errors": [],
    "inputs": [ ... ],
    "bot": { "entry_score": 70, "exit_score": 30, "position_percent": 20, "max_positions": 5, "coin_scope": "ALL", "symbols": [] },
    "run": { "candle_date": "2026-10-24", "coins_screened": 45, "coins_failed": 0, "sample_errors": [],
             "coins": [ { "rank": 1, "symbol": "SOLUSDT", "close": 142.3, "score": 90, "reason": "oversold" } ] }
} }
```

`valid = false` → tampilkan `errors` (berisi baris & posisi error sintaks).

### 1.4 Simpan perubahan script = versi baru — `POST /strategies/:id/versions`

```bash
curl -X POST $BASE/strategies/<STRATEGY_ID>/versions -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "source_code": "...script baru...",
  "inputs": [ ... ],
  "change_note": "Tambah filter volume",
  "entry_score": 75
}'
```

Field aturan bot yang tidak dikirim = sama dengan versi sebelumnya. Versi lama tetap tersimpan
(`GET /strategies/:id/versions`). User lain yang memakai versi lama **tidak** otomatis pindah —
mereka melihat `latest_version_no` dan bisa memilih upgrade.

Mengubah strategi lewat AI: kirim `strategy_id` di `/strategies/ai-chat`, lalu simpan draf dengan
endpoint ini.

### 1.5 Detail strategi — `GET /strategies/:id`

```bash
curl $BASE/strategies/<STRATEGY_ID> -H "Authorization: Bearer $TOKEN"
```

```json
{ "data": {
    "id": "...", "user_id": "...", "name": "RSI Murah Tren Naik", "description": "...",
    "kind": "BOT", "visibility": "PRIVATE", "status": "ACTIVE", "current_version_id": "...",
    "created_at": "...", "updated_at": "...",
    "is_owner": true,
    "current_version": {
      "id": "...", "version_no": 2, "language": "RHAI",
      "source_code": "...",            // HANYA untuk pemilik, user lain = null
      "inputs": [ ... ], "timeframe": "1D", "change_note": "...",
      "entry_score": 70, "exit_score": 30, "position_percent": 20, "max_positions": 3,
      "coin_scope": "SPECIFIC", "symbols": ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
    },
    "stats": {
      "official_backtest": {
        "id": "...", "period_start": "2025-10-24", "period_end": "2026-10-24",
        "stats": { "total_return_percent": 34.5, "max_drawdown_percent": -18.2, "win_rate_percent": 58.3, "profit_factor": 1.62, "...": "..." },
        "robustness": { "label": "KUAT", "...": "..." },
        "analysis": { "strengths": ["..."], "weaknesses": ["..."] },
        "validation": { "overfitting_risk": "RENDAH", "risk_score": 22, "...": "..." },
        "insights": { "top_factor": "fear_greed", "factors": [ ... ], "recommendations": [ ... ] },
        "created_at": "..."
      },
      "equity_curve": [ { "date": "2025-10-24", "equity": 1000, "drawdown_percent": 0, "btc_equity": 1000 } ],
      "live_performance": [
        { "account_mode": "DEMO", "closed_trades": 40, "win_rate_percent": 55, "realized_pnl": 120.5,
          "realized_pnl_percent": 6.1, "open_positions": 3, "open_entry_value": 300, "users": 12, "first_opened_at": "..." },
        { "account_mode": "LIVE", "...": "..." }
      ],
      "my_performance": [ ... ]
    }
} }
```

- `stats.official_backtest.validation` / `.insights` → tampilkan sama seperti [2.3](#23-uji-overfitting--validation) & [2.4](#24-faktor-paling-berpengaruh--insights).
- `stats.official_backtest = null` → versi aktif belum dibacktest. Tampilkan tombol "Jalankan Backtest".
- `live_performance` = transaksi asli **semua pemakai**, `my_performance` = transaksi **user ini**.
  1 baris per mode — tampilkan tab **DEMO** / **LIVE**. Array kosong = belum ada transaksi.
- Arti field `stats` & `robustness`: lihat [Alur 2](#alur-2--backtest).

### 1.6 Ubah, aktifkan, hapus

```bash
# ubah nama / deskripsi / status (DRAFT | ACTIVE | ARCHIVED)
curl -X PUT $BASE/strategies/<STRATEGY_ID> -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "status": "ACTIVE" }'

# hapus (ditolak kalau sedang dipasang di Store — cabut dulu)
curl -X DELETE $BASE/strategies/<STRATEGY_ID> -H "Authorization: Bearer $TOKEN"
```

Selama dipasang di Store, strategi harus tetap `ACTIVE` (status tidak bisa diubah) dan tidak bisa
dihapus — cabut dari Store dulu.

Daftar strategi milik sendiri: `GET /strategies`.

### 1.7 Skor coin hari ini — `GET /strategies/:id/results?limit=10`

```bash
curl "$BASE/strategies/<STRATEGY_ID>/results?limit=10" -H "Authorization: Bearer $TOKEN"
```

Respons sama dengan `run` di validate: daftar coin skor tertinggi (`limit` 1–50). Bisa dipakai untuk
tombol **Beli manual** (lihat [Alur 3.5](#35-beli--jual-manual-dari-hasil-strategi)).

---

## Alur 2 — Backtest

```
[Detail strategi] --tombol Backtest--> POST /strategy-backtests --> [Hasil: statistik + grafik + daftar trade]
```

### 2.1 Jalankan — `POST /strategy-backtests`

```bash
# modal default 1.000 USDT
curl -X POST $BASE/strategy-backtests -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "strategy_id": "<STRATEGY_ID>"
}'

# modal sendiri, mis. $100, dengan setelan sendiri
curl -X POST $BASE/strategy-backtests -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "strategy_id": "<STRATEGY_ID>",
  "initial_capital": 100,
  "input_values": { "rsi_period": 10 }
}'
```

| Field (opsional) | Isi |
|---|---|
| `initial_capital` | modal awal USDT, 10–1.000.000 (default 1.000). Tampilkan input "Modal" di layar backtest |
| `input_values` | setelan script sendiri (form dari `current_version.inputs`) |
| `strategy_version_id` | khusus pemilik: uji versi lama |

Proses ±2–10 detik (sudah termasuk semua uji) — tampilkan loading. Respons `201`:

```json
{ "data": {
    "id": "<BACKTEST_ID>", "strategy_id": "...", "version_no": 2, "is_official": false,
    "period_start": "2025-10-24", "period_end": "2026-10-24",
    "initial_capital": 100, "fee_rate": 0.001, "status": "DONE", "duration_ms": 1460,
    "stats": { ... }, "robustness": { ... }, "analysis": { ... },
    "validation": { ... }, "insights": { ... },
    "equity_curve": [ ... ]
} }
```

- `is_official = true` → dijalankan pemilik dengan **modal 1.000 & setelan default**. **Inilah yang
  tampil di Store** (semua strategi dibandingkan dengan syarat sama). Backtest resmi wajib ada
  sebelum strategi bisa dipasang di Store.
- Aturan simulasi: sinyal dari close hari D lalu order di open hari D+1, fee 0,1%, order < 5 USDT
  dilewati — dengan modal kecil, sebagian order bisa terlewati sehingga hasilnya berbeda dari modal
  besar (realistis).

### 2.2 Arti statistik (untuk layar hasil)

`stats`:

| Field | Tampilkan sebagai |
|---|---|
| `total_return_percent` | Total return |
| `cagr_percent` | Return per tahun |
| `max_drawdown_percent` | Penurunan modal terdalam (negatif) |
| `win_rate_percent` | % trade untung |
| `profit_factor` | Total untung ÷ total rugi (> 1 = untung) |
| `sharpe_ratio`, `sortino_ratio` | Kualitas return vs risiko |
| `volatility_percent` | Naik-turun modal per tahun |
| `trades`, `closed_trades` | Jumlah trade |
| `avg_win_percent`, `avg_loss_percent` | Rata-rata untung / rugi per trade |
| `best_trade_percent`, `worst_trade_percent` | Trade terbaik / terburuk |
| `avg_holding_days` | Rata-rata lama pegang |
| `exposure_percent` | % waktu memegang coin |
| `btc_return_percent`, `btc_max_drawdown_percent`, `beats_btc` | Pembanding beli-tahan BTC |
| `monthly_returns` | `[{ "month": "2026-01", "return_percent": 4.2 }]` — grafik batang per bulan |

`robustness` (badge ketahanan):

| Field | Arti |
|---|---|
| `label` | **KUAT** (hijau) / **SEDANG** (kuning) / **LEMAH** (merah) |
| `first_half_return_percent`, `second_half_return_percent`, `consistent_halves` | Untung di paruh awal & akhir? |
| `profitable_months_percent` | % bulan yang profit |
| `longest_drawdown_days` | Rugi terlama sebelum pulih (hari) |
| `max_consecutive_losses` | Rugi beruntun terpanjang |
| `enough_trades` | Minimal 20 trade (hasil bisa diandalkan) |
| `overfitting_risk` | sama dengan `validation.overfitting_risk` |

`analysis.strengths` / `analysis.weaknesses` — kalimat siap tampil (kelebihan ✅ / kekurangan ⚠️),
sudah termasuk hasil uji overfitting.

### 2.3 Uji overfitting — `validation`

Badge utama: `validation.overfitting_risk`

| Nilai | Tampilkan |
|---|---|
| `RENDAH` | 🟢 "Hasil kemungkinan besar bukan kebetulan" |
| `SEDANG` | 🟡 "Hati-hati, sebagian uji kurang meyakinkan" |
| `TINGGI` | 🔴 "Hasil bagus kemungkinan hanya cocok dengan data masa lalu" |
| `TIDAK_RELEVAN` | ⚪ strategi tidak profit (bukan overfit, memang belum bagus) |
| `DATA_KURANG` | ⚪ data belum cukup untuk diuji |

`risk_score` (0–100, `null` untuk 2 nilai terakhir) bisa ditampilkan sebagai meter. `few_trades =
true` → tampilkan catatan "trade sedikit, hasil kurang meyakinkan".

Tiap uji punya `available` (`false` + `reason` kalau tidak bisa dijalankan):

```json
"validation": {
  "overfitting_risk": "SEDANG", "risk_score": 48, "few_trades": true,
  "out_of_sample": {
    "available": true, "split_date": "2026-07-15",
    "in_sample":     { "from": "...", "to": "...", "return_percent": 4.1, "annual_return_percent": 5.8, "closed_trades": 9, "win_rate_percent": 55.6 },
    "out_of_sample": { "from": "...", "to": "...", "return_percent": 3.0, "annual_return_percent": 9.9, "closed_trades": 4, "win_rate_percent": 75 },
    "consistency_ratio": 1.71
  },
  "monte_carlo": {
    "available": true, "simulations": 1000, "block_days": 10,
    "return_percent":       { "worst_5": -25.9, "median": 14.19, "best_95": 91.7 },
    "max_drawdown_percent": { "worst_5": -40.69, "median": -22.84, "best_95": -12.66 },
    "probability_of_loss_percent": 32.3
  },
  "random_baseline": {
    "available": true, "runs": 300, "strategy_avg_trade_percent": 6.2,
    "random_avg_trade_percent": -1.1, "random_best_5_percent": 4.8,
    "p_value": 0.183, "beats_random_percent": 81.7
  },
  "sensitivity": {
    "available": true, "base_return_percent": 15.91,
    "variations": [
      { "parameter": "entry_score", "value": 60, "total_return_percent": 12.3, "max_drawdown_percent": -24.1, "trades": 18 },
      { "parameter": "inputs.rsi_period", "value": 11, "total_return_percent": 9.8, "max_drawdown_percent": -19.5, "trades": 14 }
    ],
    "profitable_variations_percent": 87.5, "median_return_percent": 15.91
  }
}
```

Saran tampilan per uji:

| Uji | Tampilkan |
|---|---|
| `out_of_sample` | 2 kartu "Data latihan (70%)" vs "Data uji (30%)": return/tahun & win rate. `consistency_ratio` ≥ 0,7 bagus, < 0,3 tanda overfitting |
| `monte_carlo` | "Dari 1.000 skenario: peluang rugi X%" + rentang return (5% terburuk – median – 5% terbaik) & drawdown skenario buruk |
| `random_baseline` | "Lebih baik dari `beats_random_percent`% percobaan beli acak". `p_value` ≤ 0,05 = keunggulan nyata |
| `sensitivity` | tabel/grafik batang return tiap variasi setelan — strategi stabil = sebagian besar tetap hijau |

### 2.4 Faktor paling berpengaruh — `insights`

Seperti *feature importance* di ML: faktor apa yang membedakan trade untung vs rugi.

```json
"insights": {
  "available": true, "trades_analyzed": 47, "avg_pnl_percent": -1.2, "win_rate_percent": 50,
  "top_factor": "holding_days",
  "factors": [
    {
      "key": "holding_days", "label": "Lama pegang",
      "importance_percent": 40.01, "p_value": 0.002, "significant": true, "share_percent": 63.5,
      "finding": "Lama pegang: paling bagus di 4–7 hari (rata-rata +22.3% per trade, 6 trade), paling buruk di > 30 hari (-16.7%, 21 trade)",
      "groups": [
        { "name": "4–7 hari", "trades": 6, "win_rate_percent": 83.3, "avg_pnl_percent": 22.3, "total_pnl_amount": 41.2, "enough_data": true }
      ]
    }
  ],
  "recommendations": [ "Posisi yang dipegang lebih dari 14 hari cenderung rugi — naikkan exit_score supaya keluar lebih cepat" ]
}
```

| Field | Tampilkan |
|---|---|
| `factors[]` | urut dari paling berpengaruh. Grafik batang horizontal `importance_percent` per `label`; faktor `significant = false` abu-abu ("belum terbukti") |
| `share_percent` | porsi pengaruh di antara faktor yang signifikan (untuk pie chart) |
| `finding` | kalimat di bawah tiap faktor |
| `groups[]` | detail saat faktor diketuk: tabel kelompok (jumlah trade, win rate, rata-rata return). `enough_data = false` = kurang dari 3 trade |
| `recommendations` | daftar saran 💡 |

`key` faktor: `coin`, `market_regime` (BTC 30 hari naik/sideways/turun), `fear_greed`,
`entry_score`, `holding_days`, `volatility` (ATR 14 hari), `weekday`. `available = false` (+
`reason`) kalau trade < 10.

`equity_curve` — grafik garis: `equity` (strategi) vs `btc_equity` (BTC), dan grafik area
`drawdown_percent`.

### 2.5 Riwayat, detail, daftar trade

```bash
# riwayat backtest 1 strategi (milikmu + resmi pemilik), tanpa kurva
curl "$BASE/strategy-backtests?strategy_id=<STRATEGY_ID>" -H "Authorization: Bearer $TOKEN"

# detail + kurva
curl $BASE/strategy-backtests/<BACKTEST_ID> -H "Authorization: Bearer $TOKEN"

# daftar trade simulasi (paginasi)
curl "$BASE/strategy-backtests/<BACKTEST_ID>/trades?page=1&limit=20" -H "Authorization: Bearer $TOKEN"
```

Trade: `symbol`, `entry_date`, `exit_date` (`null` = masih dipegang di akhir periode),
`entry_price`, `exit_price`, `pnl_amount`, `pnl_percent`, `holding_days`, `entry_score`,
`exit_score`, `entry_reason`, `exit_reason`.

Calon pembeli Store boleh membuka detail & trade **backtest resmi** strategi yang sedang dijual
(id dari `stats.official_backtest.id`), tanpa membeli.

---

## Alur 3 — Memakai strategi (Screener / Bot)

Strategi yang bisa dipakai: **milik sendiri**, atau **strategi Store yang sudah dibeli / trial**.

```
[Detail strategi] --"Pakai"--> pilih SCREENER / BOT --> POST /strategy-subscriptions --> [Strategi yang Dipakai]
```

### 3.1 Pakai sebagai BOT — `POST /strategy-subscriptions`

Form: pilih akun **DEMO / LIVE**, isi **% modal** (1–100), atur setelan (`inputs` versi).

```bash
curl -X POST $BASE/strategy-subscriptions -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "strategy_id": "<STRATEGY_ID>",
  "usage": "BOT",
  "account_mode": "DEMO",
  "capital_percent": 30,
  "input_values": { "rsi_period": 14 }
}'
```

- `capital_percent` = persen saldo akun yang boleh dipakai bot ini. Sisa saldo tidak disentuh.
- LIVE wajib API key Binance di profil.
- Mau DEMO **dan** LIVE → kirim 2 request (2 pemakaian).
- Bot jalan otomatis tiap hari ±01:00 UTC (08:00 WIB): jual coin skor ≤ `exit_score`, beli coin
  skor ≥ `entry_score` (maksimal `max_positions`).

### 3.2 Pakai sebagai SCREENER

```bash
curl -X POST $BASE/strategy-subscriptions -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "strategy_id": "<STRATEGY_ID>",
  "usage": "SCREENER",
  "input_values": { "rsi_period": 10 }
}'
```

Hanya untuk strategi `coin_scope = ALL`. Strategi `SPECIFIC` ditolak `400`: "Strategi untuk coin
tertentu hanya bisa dipakai sebagai BOT, bukan SCREENER" — di FE, sembunyikan pilihan SCREENER
kalau `bot.coin_scope = "SPECIFIC"`. Hasil coin: `GET /strategy-subscriptions/:id/results?limit=10`.

### 3.3 Daftar & ubah

```bash
curl $BASE/strategy-subscriptions -H "Authorization: Bearer $TOKEN"
```

Tiap item: data pemakaian (`id`, `usage`, `account_mode`, `capital_percent`, `input_values`,
`is_active`, `last_run_date`) + `strategy_name`, `version_no`, `latest_version_no`
(ada isinya = ada versi baru → tampilkan tombol "Perbarui"), `inputs` (form setelan), `bot`
(aturan robot).

```bash
# ubah modal / setelan / jeda bot
curl -X PUT $BASE/strategy-subscriptions/<SUB_ID> -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "capital_percent": 50, "is_active": false }'

# pindah ke versi terbaru
curl -X PUT $BASE/strategy-subscriptions/<SUB_ID> -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "upgrade_to_latest": true }'

# berhenti memakai
curl -X DELETE $BASE/strategy-subscriptions/<SUB_ID> -H "Authorization: Bearer $TOKEN"
```

### 3.4 Performa bot — `GET /strategy-subscriptions/:id/performance`

```bash
curl $BASE/strategy-subscriptions/<SUB_ID>/performance -H "Authorization: Bearer $TOKEN"
```

```json
{ "data": { "subscription_id": "...", "performance": [
    { "account_mode": "DEMO", "closed_trades": 12, "win_rate_percent": 58.33, "realized_pnl": 24.1,
      "realized_pnl_percent": 4.02, "open_positions": 2, "open_entry_value": 180, "users": 1, "first_opened_at": "..." }
] } }
```

Riwayat transaksi bot: `GET /transactions?filter=[{"key":"strategy_subscription_id","operator":"equal","value":"<SUB_ID>"}]`
(URL-encode parameter `filter`).

### 3.5 Beli / jual manual dari hasil strategi

Dari layar hasil (`/strategies/:id/results` atau `/strategy-subscriptions/:id/results`), user bisa
beli manual. Kirim `strategy_subscription_id` supaya transaksi tercatat sebagai performa strategi:

```bash
curl -X POST $BASE/transactions -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "transaction_type": "BUY",
  "flex_param_id": "<FLEX_PARAM_ID coin dari hasil>",
  "account_mode": "DEMO",
  "amount": 20,
  "currency": "USDT",
  "rule_type": "STRATEGY",
  "strategy_subscription_id": "<SUB_ID>"
}'
```

Jual manual — sebagian (senilai `amount`) atau **semua** coin itu dengan `coin_all: true`:

```bash
# jual sebagian, senilai 10 USDT
curl -X POST $BASE/transactions -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "transaction_type": "SELL", "flex_param_id": "<FLEX_PARAM_ID>", "account_mode": "DEMO",
  "amount": 10, "currency": "USDT"
}'

# jual SEMUA (tombol "Jual Semua") — tanpa amount, tidak tersisa walau harga bergerak
curl -X POST $BASE/transactions -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "transaction_type": "SELL", "flex_param_id": "<FLEX_PARAM_ID>", "account_mode": "DEMO",
  "coin_all": true
}'
```

- SELL menjual posisi coin itu FIFO (terlama dulu, manual maupun bot). Jual rugi tetap tercatat.
- `coin_all` hanya untuk SELL (BUY + `coin_all` → `400`). SELL tanpa `coin_all` wajib `amount`.
- LIVE: jumlah dibulatkan ke aturan Binance (`stepSize`), bisa tersisa "debu" sangat kecil.
- Kepemilikan coin & nilai sekarang: `GET /transactions/holding/<SYMBOL>?account_mode=DEMO`.

---

## Alur 4 — Store: menjual strategi (penjual)

```
Strategi ACTIVE + backtest resmi --> POST /store/listings --> [Produk Saya] --> buat promo / atur trial
```

### 4.1 Syarat sebelum dipasang

1. Strategi milik sendiri, status `ACTIVE` (`PUT /strategies/:id { "status": "ACTIVE" }`).
2. Versi aktif sudah punya backtest resmi (`POST /strategy-backtests` oleh pemilik).

### 4.2 Pasang di Store — `POST /store/listings`

```bash
curl -X POST $BASE/store/listings -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "strategy_id": "<STRATEGY_ID>",
  "title": "Bot RSI Murah Tren Naik",
  "description": "Beli coin saat koreksi di tren naik. Cocok untuk swing 1-2 minggu.",
  "pricing_model": "MONTHLY",
  "price_idr": 150000,
  "trial_days": 7
}'
```

| Field | Isi |
|---|---|
| `title` | kosong = nama strategi (maks 120 karakter) |
| `pricing_model` | `FREE` (default) · `ONE_TIME` (beli sekali, selamanya) · `MONTHLY` (30 hari) |
| `price_idr` | wajib > 0 untuk ONE_TIME / MONTHLY |
| `trial_days` | 0–90, 0 = tanpa trial |

Respons `201` = listing (bentuk sama dengan [5.2](#52-detail-produk--get-storelistingsid)).
1 strategi hanya bisa punya 1 listing.

### 4.3 Produk Saya — `GET /store/listings/mine`

```bash
curl $BASE/store/listings/mine -H "Authorization: Bearer $TOKEN"
```

Semua listing milik sendiri (termasuk `INACTIVE`), dengan statistik & `active_users` (jumlah
pemakai aktif).

### 4.4 Ubah / sembunyikan / cabut

```bash
# ubah harga, trial, atau sembunyikan (INACTIVE) / tampilkan lagi (ACTIVE)
curl -X PUT $BASE/store/listings/<LISTING_ID> -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "price_idr": 99000, "trial_days": 14, "status": "ACTIVE" }'

# cabut dari Store
curl -X DELETE $BASE/store/listings/<LISTING_ID> -H "Authorization: Bearer $TOKEN"
```

Pembeli yang hak pakainya masih berlaku tetap bisa memakai sampai habis, baik saat listing
`INACTIVE` maupun dicabut. Selama terpasang, strategi tidak bisa dihapus.

### 4.5 Promo

```bash
# buat promo
curl -X POST $BASE/store/promos -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "listing_id": "<LISTING_ID>",
  "code": "HEMAT20",
  "discount_percent": 20,
  "starts_at": "2026-11-01T00:00:00Z",
  "ends_at": "2026-11-30T23:59:59Z",
  "max_redemptions": 100
}'

# daftar promo 1 produk (khusus pemilik)
curl "$BASE/store/promos?listing_id=<LISTING_ID>" -H "Authorization: Bearer $TOKEN"

# ubah / nonaktifkan
curl -X PUT $BASE/store/promos/<PROMO_ID> -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "discount_percent": 30, "is_active": false }'

# hapus batas akhir / batas jumlah
curl -X PUT $BASE/store/promos/<PROMO_ID> -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "clear_ends_at": true, "clear_max_redemptions": true }'

# hapus promo
curl -X DELETE $BASE/store/promos/<PROMO_ID> -H "Authorization: Bearer $TOKEN"
```

| Field | Aturan |
|---|---|
| `code` | 3–30 karakter: huruf, angka, `-`, `_`. Disimpan huruf besar, unik per produk |
| `discount_percent` | 1–100 |
| `starts_at` | default sekarang (ISO 8601 UTC) |
| `ends_at` | kosong = tanpa batas, harus setelah `starts_at` |
| `max_redemptions` | kosong = tanpa batas, minimal 1 |

Respons promo berisi `redeemed_count` (sudah dipakai berapa kali). Promo tidak berlaku untuk
listing FREE dan untuk trial.

---

## Alur 5 — Store: membeli strategi (pembeli)

```
GET /store/listings --> GET /store/listings/:id --> (opsional lihat trade backtest)
   --> POST /store/purchases/quote --> POST /store/purchases --> POST /strategy-subscriptions (pakai)
```

### 5.1 Halaman Store — `GET /store/listings`

```bash
curl "$BASE/store/listings?page=1&limit=20&kind=BOT&pricing_model=MONTHLY&search=rsi" -H "Authorization: Bearer $TOKEN"
```

| Query (semua opsional) | Isi |
|---|---|
| `search` | cari di **judul listing, nama strategi, atau nama pembuat** — sebagian kata, tidak peduli huruf besar/kecil |
| `kind` | `BOT` = hanya robot, `SCREENER` = hanya screener; kosong = semua |
| `pricing_model` | `FREE` / `ONE_TIME` / `MONTHLY` |
| `page`, `limit` | paginasi (limit 1–100, default 10) |

Respons paginasi, tiap item = kartu produk (tanpa `stats.equity_curve`).

**Produk milik sendiri tidak ditampilkan** di sini — Store = dagangan user lain. Dagangan sendiri
ada di `GET /store/listings/mine` ([4.3](#43-produk-saya--get-storelistingsmine)), yang sudah dibeli di
`GET /store/purchases`, yang sedang dipakai di `GET /strategy-subscriptions`.

### 5.2 Detail produk — `GET /store/listings/:id`

```bash
curl $BASE/store/listings/<LISTING_ID> -H "Authorization: Bearer $TOKEN"
```

```json
{ "data": {
    "id": "<LISTING_ID>", "strategy_id": "...", "seller_user_id": "...",
    "title": "Bot RSI Murah Tren Naik", "description": "...",
    "pricing_model": "MONTHLY", "price_idr": 150000, "trial_days": 7, "status": "ACTIVE",
    "published_at": "...",
    "seller": { "user_id": "...", "full_name": "Budi Santoso", "photo_url": "https://..." },
    "strategy": {
      "id": "...", "name": "RSI Murah Tren Naik", "kind": "BOT", "version_no": 2, "timeframe": "1D",
      "bot": { "entry_score": 70, "exit_score": 30, "position_percent": 20, "max_positions": 3,
               "coin_scope": "SPECIFIC", "symbols": ["BTCUSDT", "ETHUSDT", "SOLUSDT"] }
    },
    "stats": {
      "official_backtest": { "id": "<BACKTEST_ID>", "stats": { ... }, "robustness": { "label": "KUAT", ... }, "analysis": { ... },
                            "validation": { "overfitting_risk": "RENDAH", ... }, "insights": { ... } },
      "equity_curve": [ ... ],
      "live_performance": [ { "account_mode": "DEMO", ... }, { "account_mode": "LIVE", ... } ],
      "my_performance": []
    },
    "active_users": 12,
    "my_access": null,
    "trial_available": true
} }
```

- **Isi script tidak pernah dikirim.** Pembuat tampil dengan nama asli & foto.
- Semua statistik dihitung sistem — penjual tidak bisa mengisinya sendiri.
- Daftar trade backtest resmi: `GET /strategy-backtests/<stats.official_backtest.id>/trades`.

Tombol di layar detail:

| Kondisi | Tombol |
|---|---|
| `my_access = null`, `trial_available = true` | **Coba Gratis N hari** + **Beli** |
| `my_access = null`, `trial_available = false` | **Beli** |
| `my_access.kind = "OWNER"` | **Kelola** (produk sendiri — muncul di detail / `/store/listings/mine`, tidak di halaman Store) |
| `my_access.kind = "PURCHASE"` / `"TRIAL"` | **Pakai** (+ "berlaku s/d `expires_at`"; `null` = selamanya). MONTHLY: **Perpanjang** |

### 5.3 Rincian harga — `POST /store/purchases/quote`

Panggil saat user membuka checkout atau memasukkan kode promo.

```bash
curl -X POST $BASE/store/purchases/quote -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{
  "listing_id": "<LISTING_ID>",
  "promo_code": "HEMAT20"
}'
```

```json
{ "data": {
    "listing_id": "...", "kind": "PURCHASE", "pricing_model": "MONTHLY",
    "list_price_idr": 150000, "discount_percent": 20, "discount_idr": 30000,
    "final_price_idr": 120000,
    "pay_now_idr": 0,
    "duration_days": 30,
    "promo_code": "HEMAT20"
} }
```

- `pay_now_idr` saat ini selalu **0** — pembayaran masih dummy. Tampilkan harga coret
  `final_price_idr` dan "Bayar Rp0 (promo peluncuran)" atau teks sejenis.
- `duration_days`: `null` = selamanya, `30` = bulanan, N = trial.
- Kode promo salah/habis → `400` dengan `message`.

Trial: `{ "listing_id": "...", "trial": true }` (tanpa `promo_code`).

### 5.4 Checkout — `POST /store/purchases`

Body sama dengan quote.

```bash
# beli
curl -X POST $BASE/store/purchases -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "listing_id": "<LISTING_ID>", "promo_code": "HEMAT20" }'

# trial
curl -X POST $BASE/store/purchases -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{ "listing_id": "<LISTING_ID>", "trial": true }'
```

Respons `201`:

```json
{ "data": {
    "id": "...", "user_id": "...", "listing_id": "...", "strategy_id": "...",
    "kind": "PURCHASE", "pricing_model": "MONTHLY",
    "list_price_idr": 150000, "discount_idr": 30000, "paid_idr": 0, "promo_id": "...",
    "payment_status": "DUMMY", "status": "ACTIVE",
    "starts_at": "...", "expires_at": "2026-11-24T08:00:00Z",
    "listing_title": "Bot RSI Murah Tren Naik",
    "is_active": true
} }
```

Hak pakai langsung aktif → arahkan ke form **Pakai** ([Alur 3](#alur-3--memakai-strategi-screener--bot))
dengan `strategy_id` dari respons.

Aturan & error yang perlu ditangani:

| Kasus | HTTP | Pesan |
|---|---|---|
| Membeli produk sendiri | 400 | "Ini strategi milikmu sendiri — langsung pakai tanpa membeli" |
| ONE_TIME / FREE sudah dimiliki | 409 | "Kamu sudah memiliki strategi ini" |
| MONTHLY masih aktif lalu beli lagi | 201 | diperpanjang 30 hari dari tanggal habis |
| Trial padahal sudah punya hak pakai | 409 | "Kamu sudah punya hak pakai strategi ini" |
| Trial kedua kali | 409 | "Trial listing ini sudah pernah kamu pakai" |
| Listing tanpa trial | 400 | "Listing ini tidak menyediakan trial" |
| Promo + trial | 400 | "Kode promo tidak berlaku untuk trial" |
| Promo di listing FREE | 400 | "Listing gratis tidak memakai kode promo" |
| Promo habis masa / kuota | 400 | "Kode promo sudah tidak berlaku (habis masa / kuota)" |
| Listing dicabut / INACTIVE | 404 | "Listing Store tidak ditemukan" |

### 5.5 Produk yang Dibeli — `GET /store/purchases`

```bash
curl $BASE/store/purchases -H "Authorization: Bearer $TOKEN"
```

Semua pembelian & trial (terbaru dulu), bentuk sama dengan respons checkout. Pakai `is_active` untuk
memisahkan tab **Aktif** / **Kedaluwarsa**. Hak pakai habis → bot dari strategi itu otomatis
berhenti dijalankan.

---

## Ringkasan 3 daftar utama

| Daftar | Endpoint | Isi |
|---|---|---|
| **Buatan saya** | `GET /strategies` (semua) · `GET /store/listings/mine` (yang dijual) | strategi milik sendiri; yang dijual bisa diatur harga, trial, promo |
| **Store** | `GET /store/listings` | semua produk ACTIVE milik **user lain** (dagangan sendiri tidak ikut); filter `kind`, cari nama robot/strategi/pembuat |
| **Sudah dibeli** | `GET /store/purchases` | hak pakai hasil beli / trial |
| **Sedang dipakai** | `GET /strategy-subscriptions` | screener & bot yang aktif (milik sendiri atau hasil beli) |
