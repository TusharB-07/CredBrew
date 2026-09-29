use sqlx::PgPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::error::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueJob {
    pub credential_id: Uuid,
    pub send_email: bool,
}

pub async fn enqueue_issue_job(pool: &PgPool, job: &IssueJob) -> Result<(), CoreError> {
    sqlx::query!(
        r#"
        INSERT INTO issue_jobs (credential_id, send_email)
        VALUES ($1, $2)
        "#,
        job.credential_id, job.send_email
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn dequeue_issue_job(pool: &PgPool) -> Result<Option<IssueJob>, CoreError> {
    let row = sqlx::query!(
        r#"
        DELETE FROM issue_jobs
        WHERE id = (
            SELECT id FROM issue_jobs
            ORDER BY created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        RETURNING credential_id, send_email
        "#
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        Ok(Some(IssueJob {
            credential_id: row.credential_id,
            send_email: row.send_email,
        }))
    } else {
        Ok(None)
    }
}

pub async fn init_queue_table(pool: &PgPool) -> Result<(), CoreError> {
    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS issue_jobs (
            id BIGSERIAL PRIMARY KEY,
            credential_id UUID NOT NULL,
            send_email BOOLEAN NOT NULL DEFAULT true,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#
    )
    .execute(pool)
    .await?;
    Ok(())
}