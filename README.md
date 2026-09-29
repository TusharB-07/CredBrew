# CredBrew — Credential Issuing Platform

A Rust + React platform for issuing verifiable badges and certificates to event participants.

## Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Frontend      │     │   API Server    │     │   Worker        │
│   (React/TS)    │────▶│   (Axum)        │────▶│   (Apalis)      │
│   Port 3000     │     │   Port 8080     │     │   Background    │
└─────────────────┘     └────────┬────────┘     └────────┬────────┘
                                 │                       │
                    ┌────────────┴────────────┐          │
                    ▼                         ▼          ▼
            ┌─────────────┐            ┌─────────────┐ ┌─────────┐
            │ PostgreSQL  │            │   MinIO/S3  │ │ Mailhog │
            │  (Data)     │            │  (Storage)  │ │ (Email) │
            └─────────────┘            └─────────────┘ └─────────┘
```

## Tech Stack

**Backend (Rust Workspace):**
- `crates/core` — Shared library: models, DB queries, render engine, storage, auth
- `crates/api` — Axum HTTP server with JWT auth, REST API
- `crates/worker` — Apalis/Postgres job queue for credential rendering & email

**Frontend:**
- Vite + React 18 + TypeScript + TanStack Query + Tailwind CSS

**Infrastructure:**
- PostgreSQL 16 (with pgcrypto)
- MinIO (S3-compatible object storage)
- Mailhog (local email testing)
- Docker Compose for local development

## Database Schema

Key tables:
- `users` — Organizer accounts
- `events` — Events owned by users
- `participant_types` — Speaker/Winner/Attendee/custom per event
- `participants` — People attending events
- `templates` — SVG templates per (type, kind) with zone config
- `credentials` — Issued credentials with status tracking
- `scan_events` — QR code scan logs

## Quick Start

### Prerequisites
- Rust 1.75+
- Node.js 18+
- Docker & Docker Compose

### 1. Start Infrastructure

```bash
docker compose up -d
```

### 2. Run Migrations

```bash
cd crates/api
cargo sqlx prepare --workspace  # optional, for compile-time query checks
sqlx migrate run --database-url postgres://credbrew:credbrew@localhost:5432/credbrew
```

### 3. Seed Demo Data

```bash
cd crates/api
cargo run --bin seed
```
This creates:
- Demo user: `demo@credbrew.dev` / `opendemo123`
- Event: "TechConf 2025"
- 3 participant types (Speaker, Winner, Attendee) with badge+cert enabled
- 6 sample participants (including "José García")
- Badge & certificate templates uploaded to MinIO

### 4. Start API Server

```bash
cd crates/api
cargo run
```
Server runs on `http://localhost:8080`

### 5. Start Worker

```bash
cd crates/worker
cargo run
```

### 6. Start Frontend

```bash
cd frontend
npm install
npm run dev
```
Frontend runs on `http://localhost:3000` (proxies `/api` to backend)

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | PostgreSQL connection string | `postgres://credbrew:credbrew@localhost:5432/credbrew` |
| `JWT_SECRET` | HS256 signing key (64+ chars) | Required |
| `S3_ENDPOINT` | MinIO/S3 endpoint | `http://localhost:9000` |
| `S3_BUCKET` | Bucket name | `credbrew` |
| `S3_ACCESS_KEY` | MinIO access key | `minioadmin` |
| `S3_SECRET_KEY` | MinIO secret key | `minioadmin` |
| `PUBLIC_BASE_URL` | Public URL for QR codes & OG tags | `http://localhost:8080` |
| `SMTP_HOST` | SMTP server | `localhost` |
| `SMTP_PORT` | SMTP port | `1025` |
| `SMTP_USER` | SMTP username | (empty) |
| `SMTP_PASS` | SMTP password | (empty) |
| `SMTP_FROM` | From address | `CredBrew <no-reply@credbrew.local>` |
| `EMAIL_PER_MIN` | Email rate limit | `60` |

## API Endpoints

### Auth
- `POST /api/auth/register` — Register new organizer
- `POST /api/auth/login` — Login, returns JWT

### Events (Owner-scoped)
- `POST /api/events` — Create event (auto-creates 3 default types)
- `GET /api/events` — List organizer's events with stats
- `GET /api/events/:id` — Get event with types & counts
- `PATCH /api/events/:id` — Update event

### Participant Types
- `POST /api/events/:id/types` — Create type
- `PATCH /api/types/:id` — Update type toggles
- `DELETE /api/types/:id` — Delete (409 if credentials exist)

### Participants
- `POST /api/events/:id/participants` — Add single participant
- `POST /api/events/:id/participants/import` — CSV import (max 5000 rows)
- `GET /api/events/:id/participants` — Paginated list with credential statuses
- `DELETE /api/participants/:id` — Delete (if no emailed/revoked creds)

### Templates
- `POST /api/templates` — Upload SVG (multipart: file, type, kind, zones)
- `GET /api/templates/:id/preview` — Rendered preview (Jane Doe + QR)
- `GET /api/templates/:id/file` — Signed S3 URL

### Issue (One Click)
- `POST /api/events/:id/issue` — `{type_id, kind: badge|certificate|both, send_email}`
  - Idempotent: re-click queues 0 new
- `GET /api/events/:id/issue-status` — Counts + recent failures
- `POST /api/credentials/:id/retry` — Re-queue failed
- `PATCH /api/credentials/:id` — Revoke (`{status: "revoked"}`)

### Public (No Auth)
- `GET /api/public/verify/:code` — JSON verification + scan log
- `GET /v/:code` — SSR HTML page with OG tags, LinkedIn buttons

## Render Engine

**Fonts:** Bundled Noto Sans Regular + Bold (via `include_bytes!`), CJK fallback via fontdb

**Pipeline:**
1. **Sanitize** — Strip scripts, event handlers, external URLs, foreignObject
2. **Inject Name** — Find `{{NAME}}` (tspan-safe), XML-escape, position from zone %
3. **Shrink-to-Fit** — Measure glyph advances via ttf-parser, scale to fit zone width (floor 40%)
4. **Inject QR** — `qrcode` crate (EcLevel::M), single `<path>` + quiet zone, replace `id="qr-zone"`
5. **Rasterize** — Badge → PNG (resvg, ≤2048px), Certificate → PDF (svg2pdf)

## Email Template

Branded HTML email with:
- Personalized greeting
- Embedded credential preview image
- **Download** button (signed S3 URL)
- **Add to LinkedIn** button (certification deep link)
- **View & Verify** button (public verify page)
- Inline CSS, 600px table layout, works in Gmail/Outlook

## LinkedIn Integration

- **Add to Profile:** `linkedin.com/profile/add?startTask=CERTIFICATION_NAME&name={Type — Event}&organizationName={Org}&issueYear={YYYY}&issueMonth={M}&certUrl={VerifyURL}`
- **Share:** `linkedin.com/sharing/share-offsite/?url={VerifyURL}`

## Development

### Run Tests

```bash
cargo test --workspace
```

### Lint

```bash
cargo clippy --workspace -- -D warnings
```

### Frontend Build

```bash
cd frontend && npm run build
```

## File Structure

```
credbrew/
├── Cargo.toml                 # Workspace root
├── docker-compose.yml         # Postgres + MinIO + Mailhog
├── .env.example               # Environment template
├── migrations/
│   └── 0001_init.sql          # Database schema
├── crates/
│   ├── core/                  # Shared library
│   │   ├── src/
│   │   │   ├── db/            # SQLx queries per entity
│   │   │   ├── render/        # SVG pipeline (sanitize, inject, raster)
│   │   │   ├── storage.rs     # S3 client
│   │   │   ├── short_code.rs  # Base62 generator
│   │   │   └── error.rs       # ApiError + IntoResponse
│   │   └── assets/
│   │       ├── fonts/         # Noto Sans (user-provided)
│   │       └── templates/     # Sample badge/cert SVGs
│   ├── api/                   # HTTP server
│   │   ├── src/
│   │   │   ├── routes/        # REST endpoints
│   │   │   ├── auth/          # JWT middleware
│   │   │   ├── templates/     # Askama verify.html
│   │   │   └── bin/seed.rs    # Demo data seeder
│   └── worker/                # Background job processor
│       ├── src/
│       │   ├── main.rs        # Apalis worker
│       │   ├── email.rs       # Branded email sender
│       └── jobs.rs            # IssueJob definition
└── frontend/
    ├── src/
    │   ├── pages/             # Login, Events, Dashboard, Participants, Templates, Issue
    │   ├── components/        # Layout, forms, tables
    │   ├── api/               # Typed fetch wrapper + TanStack Query hooks
    │   ├── stores/            # Zustand auth store
    │   └── hooks/             # useAuth, useLogin, etc.
    └── ...
```

## Acceptance Checklist

- [ ] `docker compose up` → Postgres + MinIO + Mailhog healthy
- [ ] `sqlx migrate run` succeeds
- [ ] `cargo test --workspace` all green
- [ ] Seed script runs; login with demo user works
- [ ] CSV import 100 rows with 3 bad → imports 97 + reports 3 with row numbers
- [ ] Template missing `{{NAME}}` → 422 with clear message
- [ ] Preview returns PNG showing "Jane Doe" + scannable QR
- [ ] Issue endpoint: first call queues N, second call queues 0/skips N
- [ ] Worker renders all → statuses become `emailed`; Mailhog shows emails
- [ ] Name "José García & Co <3" renders escaped correctly
- [ ] Long name shrinks to fit; CJK name falls back without tofu
- [ ] QR scan opens `/v/{code}` — shows ✅ Verified, correct data, scan recorded
- [ ] Verify page source has `og:title` + `og:image` tags
- [ ] LinkedIn "Add" deep link prefills name, org, date, certUrl
- [ ] Retry flow: bad SMTP → failed → retry succeeds
- [ ] Revoke → 410 on verify page
- [ ] `cargo clippy --workspace -- -D warnings` clean

## License

MIT