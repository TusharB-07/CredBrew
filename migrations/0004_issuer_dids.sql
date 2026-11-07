-- Migration 0004: DID + signing infrastructure
-- Adds issuer_dids, signing_keys, and status_lists tables.
-- See: CredBrew v1.0 Design RFC §4, §7, §10.1

-- ─────────────────────────────────────────────────────────────────────────────
-- Issuer DIDs: one or more did:web identifiers per tenant.
-- A tenant's "active" DID is the one referenced by verificationMethod in
-- newly-issued credentials. Older DIDs are retained so previously-issued
-- credentials remain verifiable.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE issuer_dids (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  did TEXT UNIQUE NOT NULL,  -- e.g. did:web:credbrew.example:tenants:techconf
  method TEXT NOT NULL DEFAULT 'web' CHECK (method IN ('web','key','ion','tz')),
  did_document JSONB NOT NULL,  -- the full DID document, served at /.well-known/...
  is_active BOOLEAN NOT NULL DEFAULT true,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_issuer_dids_tenant ON issuer_dids(tenant_id);
CREATE UNIQUE INDEX idx_issuer_dids_one_active_per_tenant
  ON issuer_dids(tenant_id) WHERE is_active = true;

-- ─────────────────────────────────────────────────────────────────────────────
-- Signing keys: Ed25519 keypairs owned by an issuer DID.
-- The encrypted_seed column holds the 32-byte Ed25519 seed sealed with the
-- tenant's KMS master key. The master key never touches the database.
--
-- Status lifecycle:
--   active    → in use for signing new credentials
--   retired   → no longer used for new signatures, but kept resolvable so
--               old credentials remain verifiable
--   revoked   → removed from the DID document; credentials signed by this
--               key will fail verification (key compromise scenario)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE signing_keys (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  issuer_did_id UUID NOT NULL REFERENCES issuer_dids(id) ON DELETE CASCADE,
  key_id TEXT NOT NULL,  -- the #fragment in the DID document, e.g. "key-1"
  public_key_multibase TEXT NOT NULL,  -- z-prefix base58btc (multibase 'z' + base58)
  encrypted_seed BYTEA NOT NULL,  -- 32 bytes, sealed with KMS master key
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','retired','revoked')),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  retired_at TIMESTAMPTZ
);
CREATE INDEX idx_signing_keys_did ON signing_keys(issuer_did_id);
CREATE UNIQUE INDEX idx_signing_keys_one_active_per_did
  ON signing_keys(issuer_did_id) WHERE status = 'active';

-- ─────────────────────────────────────────────────────────────────────────────
-- Status lists: BitstringStatusList 2021 (W3C) for credential revocation.
-- One list per tenant, 16384 bytes (131072 bits). When a list fills, a new
-- row with index+1 is created. The list itself is re-signed as a separate
-- Verifiable Credential (the StatusListCredential) and served at
-- /tenants/{slug}/status/{index}.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE status_lists (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  index INT NOT NULL,  -- 1, 2, 3... per tenant
  bitstring BYTEA NOT NULL DEFAULT '\x00'::bytea || repeat('\x00', 16383)::bytea,
  vc_id TEXT NOT NULL,  -- the StatusListCredential URL
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(tenant_id, index)
);
CREATE INDEX idx_status_lists_tenant ON status_lists(tenant_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- Add vc_json + status list reference to credentials.
-- vc_json is the signed Verifiable Credential (source of truth).
-- file_key (PNG) is now a derived artifact.
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE credentials
  ADD COLUMN vc_id TEXT,                    -- the VC's id field (URL)
  ADD COLUMN vc_json JSONB,                 -- the full signed VC document
  ADD COLUMN status_list_id UUID REFERENCES status_lists(id),
  ADD COLUMN status_list_index INT,         -- bit position in the list
  ADD COLUMN anchored_chain TEXT,           -- 'polygon' / 'arweave' / NULL
  ADD COLUMN anchored_txid TEXT,
  ADD COLUMN anchored_at TIMESTAMPTZ;
CREATE INDEX idx_credentials_status_list ON credentials(status_list_id) WHERE status_list_id IS NOT NULL;
