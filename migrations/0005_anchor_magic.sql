-- Migration 0005: Anchor jobs + magic-link auth
-- See: CredBrew v1.0 Design RFC §8 (anchoring), §9 (recipient experience)

-- ─────────────────────────────────────────────────────────────────────────────
-- Anchor jobs: queued on-chain anchoring requests, drained by credbrew-anchor.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE anchor_jobs (
  id BIGSERIAL PRIMARY KEY,
  credential_id UUID NOT NULL REFERENCES credentials(id) ON DELETE CASCADE,
  chain TEXT NOT NULL DEFAULT 'polygon' CHECK (chain IN ('polygon','arweave')),
  credential_hash TEXT NOT NULL,  -- SHA-256 of the canonicalized VC, hex-encoded
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','submitted','confirmed','failed')),
  tx_hash TEXT,
  block_number BIGINT,
  batch_id BIGINT,  -- groups jobs submitted in the same tx
  error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  submitted_at TIMESTAMPTZ,
  confirmed_at TIMESTAMPTZ
);
CREATE INDEX idx_anchor_jobs_status ON anchor_jobs(status);
CREATE INDEX idx_anchor_jobs_credential ON anchor_jobs(credential_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- Magic links: single-use, short-TTL tokens for recipient auth.
-- The /my/credentials page uses these instead of passwords — recipients are
-- credential holders, not users of CredBrew.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE magic_links (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email TEXT NOT NULL,
  token TEXT NOT NULL UNIQUE,  -- the secret in the magic-link URL
  expires_at TIMESTAMPTZ NOT NULL,
  used_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_magic_links_token ON magic_links(token);
CREATE INDEX idx_magic_links_email ON magic_links(email);

-- ─────────────────────────────────────────────────────────────────────────────
-- Track which credentials a recipient email has been issued (denormalized for
-- fast /my/credentials lookups). The credentialSubject.id is mailto:{email},
-- so we can query by email directly.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE INDEX idx_participants_email_lower ON participants(LOWER(email));
