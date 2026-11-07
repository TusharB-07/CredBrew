-- Migration 0003: Multi-tenancy foundation
-- Adds tenants, tenant_memberships, and tenant_id scoping to all issuer-owned tables.
-- See: CredBrew v1.0 Design RFC §10.1

-- ─────────────────────────────────────────────────────────────────────────────
-- Tenants: an organization/workspace that owns events and credentials.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE tenants (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  slug TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  domain TEXT,  -- nullable; set when BYO-domain is configured (v1.1+)
  plan TEXT NOT NULL DEFAULT 'free' CHECK (plan IN ('free','pro','enterprise')),
  anchoring_enabled BOOLEAN NOT NULL DEFAULT false,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_tenants_slug ON tenants(slug);

-- ─────────────────────────────────────────────────────────────────────────────
-- Tenant memberships: many-to-many between users and tenants with roles.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE tenant_memberships (
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner','admin','member')),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, tenant_id)
);
CREATE INDEX idx_memberships_user ON tenant_memberships(user_id);
CREATE INDEX idx_memberships_tenant ON tenant_memberships(tenant_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- Add tenant_id to all issuer-owned tables.
-- Nullable for backward compatibility during the v0.1 → v1.0 migration window.
-- A follow-up migration (0004) will set NOT NULL after the backfill binary runs.
-- ─────────────────────────────────────────────────────────────────────────────

ALTER TABLE events
  ADD COLUMN tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
CREATE INDEX idx_events_tenant ON events(tenant_id);

ALTER TABLE participant_types
  ADD COLUMN tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
CREATE INDEX idx_participant_types_tenant ON participant_types(tenant_id);

ALTER TABLE participants
  ADD COLUMN tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
CREATE INDEX idx_participants_tenant ON participants(tenant_id);

ALTER TABLE templates
  ADD COLUMN tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
CREATE INDEX idx_templates_tenant ON templates(tenant_id);

ALTER TABLE credentials
  ADD COLUMN tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
CREATE INDEX idx_credentials_tenant ON credentials(tenant_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- Helpful view: user's accessible tenants (for the tenant switcher UI).
-- ─────────────────────────────────────────────────────────────────────────────
CREATE OR REPLACE VIEW user_tenants AS
SELECT
  u.id AS user_id,
  t.id AS tenant_id,
  t.slug,
  t.name,
  tm.role,
  tm.created_at AS member_since
FROM users u
JOIN tenant_memberships tm ON tm.user_id = u.id
JOIN tenants t ON t.id = tm.tenant_id;
