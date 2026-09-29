# CredBrew — Project Analysis

## 1. Overview

CredBrew is a **verifiable credential issuing platform** for events. Organizers create events, add participants (single or CSV import), upload SVG badge/certificate templates, and issue credentials with one click. Each credential embeds a scannable QR code pointing at a public verification page. Emails with download/LinkedIn/verify links are sent to participants. The platform is built as a **Rust workspace backend + React frontend**, with PostgreSQL, S3-compatible storage (MinIO), and SMTP (Mailhog in dev).

**Scale:** ~4,400 lines of Rust + ~2,200 lines of TypeScript/TSX, 2 SQL migrations, dockerized local infra.

---

## 2. Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌────────────────┐
│  Frontend       │    │  API (Axum)     │    │  Worker        │
│  React/Vite     │───▶│  :8080          │───▶│  poller :5s    │
│  :3000 (proxy)  │    │  JWT auth       │    │  render+email  │
└─────────────────┘    └───────┬─────────┘    └───────┬────────┘
                              │                      │
                ┌─────────────┴─────────────┐         │
                ▼                           ▼         ▼
         PostgreSQL (data + job queue)   MinIO/S3  SMTP/Mailhog
```

- **`crates/core`** — shared library: models, DB access, render pipeline, storage, short codes, error types.
- **`crates/api`** — Axum HTTP server: JWT auth, REST routes, SSR verify page (Askama), public verification endpoints.
- **`crates/worker`** — background processor: dequeues issue jobs, renders SVG→PNG, uploads to S3, sends emails.
- **`frontend/`** — Vite + React 18 + TanStack Query + Zustand + Tailwind.

The two binaries share the same `credbrew_core` crate and both run migrations at startup (`sqlx::migrate!`).

---

## 3. Data Model

Nine tables (2 migrations):

| Table | Purpose | Key constraints |
|-------|---------|-----------------|
| `users` | organizer accounts | Argon2 password hash |
| `events` | owned by a user, has accent color / org name / date | owner FK cascade |
| `participant_types` | Speaker/Winner/Attendee/custom per event, badge+cert toggles | unique `(event_id, name)` |
| `participants` | people in an event | unique `(event_id, email)`; unique `short_code` |
| `templates` | one SVG per `(participant_type_id, kind in badge/certificate)`, JSONB `name_zone` + `qr_zone` | unique `(participant_type_id, kind)`, upsert on save |
| `credentials` | issued credential, status machine `pending→rendered→emailed / failed / revoked` | unique `(participant_id, template_id)` |
| `scan_events` | QR scan audit log | indexed on `short_code` |
| `issue_jobs` | custom queue table | populated by API, drained by worker |

The credential status enum is enforced both via a DB CHECK constraint and a Rust enum (`CredentialStatus`) mapped with `sqlx::Type`. DB constraints are the source of truth.

---

## 4. Core Flows

### 4.1 Credential issuance (one-click)
1. `POST /api/events/:id/issue` → validates ownership → resolves templates → `INSERT ... SELECT ... ON CONFLICT DO NOTHING` per kind (idempotent credential creation) → enqueues one `issue_jobs` row per credential.
2. Worker polls every 5s; **dequeues with `DELETE ... FOR UPDATE SKIP LOCKED`** (a lightweight, Postgres-native FIFO queue — no external broker).
3. Worker loads credential + join details, skips `emailed`/`revoked`, reads SVG from S3, runs the render pipeline, uploads PNG, sets status `rendered`, then optionally sends email and sets `emailed`.

### 4.2 Render pipeline (crates/core/src/render)
1. **Sanitize** (`sanitize.rs`) — quick-xml rewrite stripping `<script>`, `foreignObject`, `on*` handlers, external `href/xlink:href`, oversized data-URIs (>500KB). Unit tested.
2. **Inject name** (`inject_name.rs`) — replaces `{{NAME}}` (tspan-safe), XML-escapes (`& < >`), then re-parses with usvg.
3. **Shrink-to-fit** (`shrink_to_fit.rs`) — measures glyph advances via `ttf-parser` to scale `font-size` down to fit the zone width (floor at 40% of base size).
4. **Inject QR** (`inject_qr.rs`) — `qrcode` crate at `EcLevel::M`, encodes `{base}/v/{code}`, replaces element `id="qr-zone"` with a single `<path>` + white quiet-zone rect in a transformed `<g>`. Deterministic output (tested).
5. **Rasterize** (`raster.rs`) — Badges: PNG via resvg capped at 2048px. Certificates: **also PNG** at ~300 DPI-equivalent scale. Text is converted to paths using bundled Noto Sans Regular/Bold.

### 4.3 Verification
- QR scans open `/v/{code}` → Askama server-rendered HTML with OpenGraph/Twitter meta tags, "Verified" UI, download + LinkedIn add/share buttons. `Cache-Control: public, max-age=300`.
- `GET /api/public/verify/:code` returns JSON and logs a scan row.
- `GET /api/public/verify/asset/:code` and `/api/public/og-image/:code` stream the stored PNG.
- Revoked credentials return an error on the verify page.

### 4.4 Email (crates/worker/src/email.rs)
Branded 600px table HTML (inline styles, `%%` escaping) with embedded preview image, Download, Add to LinkedIn, and View & Verify buttons. Sent via lettre; rate-limited by a governor `Quota::per_minute(EMAIL_PER_MIN)`.

### 4.5 Auth
`POST /auto/auth/register` + `/login` → Argon2 hashing/verify → HS256 JWT (`sub` = user id, default 24h expiry). A `from_fn` middleware decodes the token, loads the user, inserts into request extensions; handlers receive `AuthUser` extractor. Password min length 8; JWT secret min 32 chars with a warning for the known dev value.

---

## 5. Components / Observations

### Strengths
- Clean **workspace split** (core/api/worker) with shared versions in root `Cargo.toml`.
- **Compile-time checked SQL** via `sqlx::query!` — the `.sqlx/` dir caches 46 prepared queries.
- **SVG sanitization is serious** — strips scripts/handlers/external refs before storage and validation requires both `{{NAME}}` and `id="qr-zone"` placeholders.
- Idempotent issuance (`ON CONFLICT DO NOTHING`) prevents duplicate credential rows on re-click.
- Owner/event scoping checks on essentially every protected route.
- Lots of focused unit tests (sanitize, inject, QR determinism, shrink-to-fit, auth token, short codes).
- Well-documented README with acceptance checklist.

### Discrepancies between README and code
| README says | Code does |
|---|---|
| Apalis job queue | Custom `issue_jobs` table + `SELECT ... FOR UPDATE SKIP LOCKED` poll loop. No `apalis` dependency exists. |
| Certificate → PDF (`svg2pdf`) | Certificates are rendered as **PNG** via resvg; `svg2pdf` is not a dependency. |
| `GET /api/templates/:id/file` → signed S3 URL | Returns a 302 redirect to `/api/files/templates/{file_key}`, which streams the file after an ownership check. |

### Bugs / gaps worth fixing
1. **Worker never marks a credential `failed`.** `dequeue_issue_job` deletes the row before work; `process_job` returns `Err` → main loop only logs. The credential stays `pending` forever and `last_error`/`attempts` are never set. The entire **retry flow is unreachable** (`retry_credential` requires `status == failed`). Failure tracking needs a status update before the error is dropped.
2. **`IssueResponse.skipped` is always `0`** (`issue.rs:84`), even though idempotency exists — the re-click "skips N" is never reported.
3. **Event stats are placeholders.** `list_events`/`get_event` hardcode `total_participants`, `credentials_emailed`, `credentials_failed`, `badges_issued`, `certificates_issued` to `0` (marked TODO). Also computed per-type with N+1 queries and `.unwrap()` on template lookups that would **panic** on DB error (`events.rs:87-88`).
4. **Participant credential statuses never surface.** `list_participants` sets `badge_status`/`certificate_status` to `None` and queries `get_credential_by_participant_and_template(id, Uuid::nil())` which can never match (template_id is nil), so the UI can't show issuance state per participant.
5. **Revoked → HTTP 500.** `public.rs` (`verify_credential`, `verify_page`) and `files.rs` return `CoreError::Internal` for revoked credentials → `500 ISE` instead of `410 Gone` (README claims 410). `verify_credential` also logs a scan **before** the existence/revocation check, writing scan rows for invalid codes.
6. **`og_image` doesn't check revoked status**, so a revoked credential's image remains fetchable.
7. **Worker throughput**: only one job per 5s poll tick — no concurrency, no batch. Fine for small volumes, not for thousands of rows quickly.
8. **CSV size guard is heuristic** (`5_000 * 200` bytes) and imported within a single request; no transaction, so partial imports are expected behavior (documented) but there are no per-row DB race protections beyond unique constraints.
9. **No email validation on register** and no password confirmation/rate-limit nuances beyond a global auth limiter; auth limiter is shared and strict (5/s, burst 5).
10. **Unused code**: `storage.public_url()` is never called; `scan_events::get_scans_by_code` unused; `token-stream`/`aws-sdk-s3` in Cargo.toml appear unused; stray `crates/api/package-lock.json` artifact.

### Security notes
- Rate limiters via `tower_governor`: 5/s on protected routes, 5/s burst 5 on auth, 30/s on some public routes. **Not applied** to `/v/:code`, `/api/public/verify/asset/:code`.
- SVG sanitizer blocks the typical XXE/script vectors; XML-escaping prevents name injection (`José García & Co <3` handled per README checklist).
- Passwords Argon2id salted; JWT HS256 with length-checked secret.
- Signed storage is dev MinIO with `with_allow_http(true)` — production must configure TLS/real S3.

---

## 6. File Layout (key paths)

```
crates/core/src/
  models.rs           entities + enums (TemplateKind, CredentialStatus) + zones
  error.rs            CoreError enum + IntoResponse mapping
  short_code.rs       base62, 8-char unique code generator (5 retries)
  storage.rs          object_store/S3 wrapper
  queue.rs            enqueue/dequeue (SKIP LOCKED) + init_queue_table
  render/             sanitize, inject_name, inject_qr, shrink_to_fit, raster
  db/                 per-entity SQLx query modules
crates/api/src/
  main.rs             router, 3 rate limiters, middleware wiring
  state.rs            AppConfig from env + dev-secret warning
  routes/             auth, events, participant_types, participants,
                      templates, files, issue, public
  templates/verify.html
  bin/seed.rs         demo user/event/participants/templates + JWT printout
crates/worker/src/
  main.rs             poll loop + process_job
  email.rs            branded HTML email via lettre
frontend/src/
  api/client.ts       typed fetch wrapper + TanStack hooks
  stores/auth.ts, hooks/useAuth.ts
  pages/              Login, Register, EventList, EventCreate, EventDashboard,
                      Participants, Templates, Issue
```

---

## 7. Frontend Summary

- Vite dev server on :3000 proxying `/api` → :8080. React 18 + react-router v6 + TanStack Query v5 + Zustand + Tailwind.
- Auth token in `localStorage`, 401 responses clear it and redirect to `/login`.
- Pages: Login/Register, Event create/list, event dashboard with per-type stats, participant listing + CSV import, template upload with zone config (name/QR placement), and a one-click Issue screen.
- Frontend types (`src/types`) mirror backend DTOs.

---

## 8. Development Environment

- `docker compose up -d` → PostgreSQL 16 (pgcrypto), MinIO (auto-creates `credbrew` bucket via `mc`), Mailhog (:1025 SMTP, :8025 UI).
- `sqlx migrate run` or auto-migrate on server start.
- `cargo run --bin seed` → demo organizer (`demo@credbrew.dev` / `opendemo123`), "TechConf 2025", 3 types, 7 participants, badge + cert templates for every type, prints a JWT.
- Verify: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `npm run build`.

---

## 9. Summary Assessment

A compact, well-structured MVP for issuing and verifying event credentials. The render pipeline (sanitize → inject → shrink → QR → rasterize) is the standout piece and is well tested. The most impactful next steps are: (1) make the worker actually record `failed` statuses so retries work, (2) fill in the placeholder stats and participant credential-status fields, (3) return `410 Gone` for revoked credentials, and (4) fix the `skipped` count on issuance.