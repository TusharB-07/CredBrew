CREATE TABLE IF NOT EXISTS issue_jobs (
    id BIGSERIAL PRIMARY KEY,
    credential_id UUID NOT NULL,
    send_email BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);