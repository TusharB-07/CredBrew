//! On-chain anchoring job queue. The credbrew-anchor binary polls this table,
//! batches pending jobs, submits them to the chain, and updates status on
//! confirmation.
//!
//! See: CredBrew v1.0 Design RFC §8

use crate::error::CoreError;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum AnchorStatus {
    Pending,
    Submitted,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct AnchorJob {
    pub id: i64,
    pub credential_id: Uuid,
    pub chain: String,
    pub credential_hash: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub block_number: Option<i64>,
    pub batch_id: Option<i64>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub confirmed_at: Option<DateTime<Utc>>,
}

/// Enqueue an anchoring job for a credential. Called by the worker when the
/// tenant has anchoring_enabled. Idempotent: if a pending job already exists
/// for this credential, does nothing.
pub async fn enqueue(
    pool: &PgPool,
    credential_id: Uuid,
    chain: &str,
    credential_hash: &str,
) -> Result<(), CoreError> {
    sqlx::query(
        r#"
        INSERT INTO anchor_jobs (credential_id, chain, credential_hash)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(credential_id)
    .bind(chain)
    .bind(credential_hash)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetch up to `limit` pending jobs for a given chain, atomically marking
/// them as submitted with a shared batch_id. Returns the jobs + the batch_id.
/// This is the dequeue primitive the anchor binary calls each tick.
pub async fn dequeue_batch(
    pool: &PgPool,
    chain: &str,
    limit: i64,
) -> Result<(Vec<AnchorJob>, i64), CoreError> {
    // Create the sequence if it doesn't exist (first run).
    let _ = sqlx::query("CREATE SEQUENCE IF NOT EXISTS anchor_jobs_batch_seq")
        .execute(pool)
        .await;
    let batch_id: i64 = sqlx::query_scalar("SELECT nextval('anchor_jobs_batch_seq')")
        .fetch_one(pool)
        .await
        .unwrap_or(1);

    // Atomically claim up to `limit` pending jobs.
    let rows = sqlx::query(
        r#"
        UPDATE anchor_jobs
        SET status = 'submitted',
            submitted_at = now(),
            batch_id = $3
        WHERE id IN (
            SELECT id FROM anchor_jobs
            WHERE status = 'pending' AND chain = $1
            ORDER BY created_at
            LIMIT $2
            FOR UPDATE SKIP LOCKED
        )
        RETURNING id, credential_id, chain, credential_hash, status, tx_hash,
                  block_number, batch_id, error, created_at, submitted_at, confirmed_at
        "#,
    )
    .bind(chain)
    .bind(limit)
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    let jobs: Vec<AnchorJob> = rows
        .into_iter()
        .map(|r| AnchorJob {
            id: r.get("id"),
            credential_id: r.get("credential_id"),
            chain: r.get("chain"),
            credential_hash: r.get("credential_hash"),
            status: r.get("status"),
            tx_hash: r.get("tx_hash"),
            block_number: r.get("block_number"),
            batch_id: r.get("batch_id"),
            error: r.get("error"),
            created_at: r.get("created_at"),
            submitted_at: r.get("submitted_at"),
            confirmed_at: r.get("confirmed_at"),
        })
        .collect();

    Ok((jobs, batch_id))
}

/// Mark a batch as confirmed with the tx hash + block number.
pub async fn confirm_batch(
    pool: &PgPool,
    batch_id: i64,
    tx_hash: &str,
    block_number: i64,
) -> Result<u64, CoreError> {
    let result = sqlx::query(
        r#"
        UPDATE anchor_jobs
        SET status = 'confirmed',
            tx_hash = $2,
            block_number = $3,
            confirmed_at = now()
        WHERE batch_id = $1
        "#,
    )
    .bind(batch_id)
    .bind(tx_hash)
    .bind(block_number)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Mark a batch as failed with an error message.
pub async fn fail_batch(pool: &PgPool, batch_id: i64, error: &str) -> Result<u64, CoreError> {
    let result = sqlx::query(
        r#"
        UPDATE anchor_jobs
        SET status = 'failed',
            error = $2
        WHERE batch_id = $1
        "#,
    )
    .bind(batch_id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Get the anchor info for a credential (for the verify page's "anchored on
/// chain" link).
pub async fn get_for_credential(
    pool: &PgPool,
    credential_id: Uuid,
) -> Result<Option<AnchorJob>, CoreError> {
    let row = sqlx::query(
        r#"
        SELECT id, credential_id, chain, credential_hash, status, tx_hash,
               block_number, batch_id, error, created_at, submitted_at, confirmed_at
        FROM anchor_jobs
        WHERE credential_id = $1 AND status = 'confirmed'
        ORDER BY confirmed_at DESC
        LIMIT 1
        "#,
    )
    .bind(credential_id)
    .fetch_optional(pool)
    .await?;

    if let Some(r) = row {
        Ok(Some(AnchorJob {
            id: r.get("id"),
            credential_id: r.get("credential_id"),
            chain: r.get("chain"),
            credential_hash: r.get("credential_hash"),
            status: r.get("status"),
            tx_hash: r.get("tx_hash"),
            block_number: r.get("block_number"),
            batch_id: r.get("batch_id"),
            error: r.get("error"),
            created_at: r.get("created_at"),
            submitted_at: r.get("submitted_at"),
            confirmed_at: r.get("confirmed_at"),
        }))
    } else {
        Ok(None)
    }
}
