CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE users (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  event_date DATE,
  org_name TEXT NOT NULL DEFAULT '',
  accent_color TEXT NOT NULL DEFAULT '#4F46E5',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE participant_types (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  event_id UUID NOT NULL REFERENCES events(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  badge_enabled BOOLEAN NOT NULL DEFAULT false,
  certificate_enabled BOOLEAN NOT NULL DEFAULT false,
  UNIQUE(event_id, name)
);

CREATE TABLE participants (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  event_id UUID NOT NULL REFERENCES events(id) ON DELETE CASCADE,
  participant_type_id UUID NOT NULL REFERENCES participant_types(id),
  name TEXT NOT NULL,
  email TEXT NOT NULL,
  short_code TEXT UNIQUE NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(event_id, email)
);
CREATE INDEX idx_participants_event ON participants(event_id);
CREATE INDEX idx_participants_type ON participants(participant_type_id);

CREATE TABLE templates (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  participant_type_id UUID NOT NULL REFERENCES participant_types(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('badge','certificate')),
  file_key TEXT NOT NULL,
  name_zone JSONB NOT NULL,
  qr_zone JSONB NOT NULL,
  UNIQUE(participant_type_id, kind)
);

CREATE TABLE credentials (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  participant_id UUID NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
  template_id UUID NOT NULL REFERENCES templates(id),
  kind TEXT NOT NULL CHECK (kind IN ('badge','certificate')),
  file_key TEXT,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','rendered','emailed','failed','revoked')),
  attempts INT NOT NULL DEFAULT 0,
  last_error TEXT,
  issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(participant_id, template_id)
);
CREATE INDEX idx_credentials_status ON credentials(status);

CREATE TABLE scan_events (
  id BIGSERIAL PRIMARY KEY,
  short_code TEXT NOT NULL,
  scanned_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_scan_code ON scan_events(short_code);