# CredBrew — Verifiable Credential Issuing Platform

A self-hostable, open-source alternative to Credly for issuing **cryptographically verifiable** badges and certificates. Every credential is a W3C Verifiable Credential signed with the issuer's own Ed25519 key, anchored to a did:web identifier, and optionally timestamped on-chain (Polygon). Recipients own their credentials — they can export them to any OB 3.0-compatible wallet (Badgr, Spruce, Disco) and verify them independently of CredBrew's servers.

**v1.0 status:** Phases 1–6 complete (multi-tenancy, DIDs + signing, VC issuance, revocation + status lists, recipient wallet with magic-link auth, optional on-chain anchoring).

## Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Frontend      │     │   API Server    │     │   Worker        │     │   Anchor        │
│   (React/TS)    │────▶│   (Axum)        │────▶│   (VC signing)  │────▶│   (Polygon RPC) │
│   Port 3000     │     │   Port 8080     │     │   Background    │     │   Optional      │
└─────────────────┘     └────────┬────────┘     └────────┬────────┘     └────────┬────────┘
                                 │                       │                       │
                    ┌────────────┴────────────┐          │                       │
                    ▼                         ▼          ▼                       ▼
            ┌─────────────┐            ┌─────────────┐ ┌─────────┐       ┌───────────┐
            │ PostgreSQL  │            │   MinIO/S3  │ │ Mailhog │       │ Polygon   │
            │ (Data+Queue)│            │  (Storage)  │ │ (Email) │       │ (On-chain)│
            └─────────────┘            └─────────────┘ └─────────┘       └───────────┘
```

**Trust model:** Credentials are signed with Ed25519 (Ed25519Signature2020 LD-Proof). The issuer's public key is published in a did:web DID document served at `/.well-known/did.json` or `/tenants/{slug}/did.json`. Verification = fetch the DID document, extract the public key, verify the signature. **CredBrew's server is not required for verification** — the credential is self-contained.

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
- `tenants` — Workspaces/organizations (multi-tenancy, v1.0)
- `tenant_memberships` — User × tenant × role (owner/admin/member)
- `events` — Events owned by users, scoped to a tenant
- `participant_types` — Speaker/Winner/Attendee/custom per event
- `participants` — People attending events
- `templates` — SVG templates per (type, kind) with zone config
- `credentials` — Issued credentials with status tracking
- `scan_events` — QR code scan logs
- `issuer_dids` — Tenant's did:web identifiers (v1.0 Phase 2)
- `signing_keys` — Ed25519 keypairs, sealed at rest (v1.0 Phase 2)
- `status_lists` — BitstringStatusList 2021 for revocation (v1.0 Phase 4)

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
| `CREDBREW_MASTER_KEY` | KMS master key for sealing Ed25519 signing seeds (16+ chars) | Required (v1.0+) |

### On-Chain Anchoring (Phase 6, optional — only if `tenants.anchoring_enabled = true`)

| Variable | Description |
|----------|-------------|
| `ETH_RPC_URL` | Polygon RPC endpoint (e.g. `https://polygon-rpc.com`) |
| `ANCHOR_PRIVATE_KEY` | Funded wallet's private key (hex, no 0x prefix) |
| `ANCHOR_WALLET_ADDRESS` | The wallet's ETH address (hex with 0x) |

## Binaries

CredBrew ships four binaries:

```bash
cargo run --bin credbrew-api       # HTTP server (port 8080)
cargo run --bin credbrew-worker    # Background VC signing + PNG rendering + email
cargo run --bin credbrew-anchor    # Optional: on-chain anchoring (Polygon RPC)
cargo run --bin seed               # Demo data: demo user + TechConf 2025 + sample participants
cargo run --bin migrate_to_v1      # One-time v0.1 → v1.0 migration (creates tenants, backfills tenant_id)
```

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
- `GET /v/:code` — SSR HTML page with proof block, DID, wallet dropdown, chain link, LinkedIn buttons
- `GET /api/public/vc/:code` — Download the raw Verifiable Credential (.vc.json) for wallet import
- `GET /tenants/:slug/did.json` — DID document for did:web resolution (sub-path tenants)
- `GET /.well-known/did.json` — DID document for root-domain self-hosted tenants
- `GET /tenants/:slug/status/:index` — Signed StatusListCredential (BitstringStatusList 2021) for revocation checking

### Recipient Auth (v1.0 Phase 5 — Magic Link)
- `POST /api/auth/magic-link` — Request a magic link emailed to a recipient (always returns 200 to prevent email enumeration)
- `GET /api/auth/magic-link/verify?token=...` — Exchange a magic link token for a recipient session, redirects to /my/credentials
- `GET /my/credentials?session=...&email=...` — Recipient's read-only credential wallet (all credentials issued to their email)

### DID + Signing (v1.0 Phase 2 — Tenant Admin)
- `POST /api/tenants/me/did` — Mint a new did:web identifier + Ed25519 keypair for the active tenant
- `GET /api/tenants/me/did` — Get the active issuer DID + signing key
- `GET /api/tenants/me/keys` — List all signing keys (active/retired/revoked)
- `POST /api/tenants/me/keys/:id/retire` — Retire a signing key (keeps old credentials verifiable)
- `POST /api/tenants/me/keys/:id/revoke` — Revoke a signing key (key compromise; old credentials fail verification)
- `POST /api/tenants/me/sign-test` — Sign a sample VC with the tenant's active key, verify it back, return the signed VC + proof

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

