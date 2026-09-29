use sqlx::PgPool;
use crate::models::ScanEvent;
use crate::error::CoreError;

pub async fn log_scan(pool: &PgPool, short_code: &str) -> Result<ScanEvent, CoreError> {
    let scan = sqlx::query_as!(
        ScanEvent,
        r#"
        INSERT INTO scan_events (short_code)
        VALUES ($1)
        RETURNING id, short_code, scanned_at
        "#,
        short_code
    )
    .fetch_one(pool)
    .await?;
    Ok(scan)
}

pub async fn get_scans_by_code(pool: &PgPool, short_code: &str) -> Result<Vec<ScanEvent>, CoreError> {
    let scans = sqlx::query_as!(
        ScanEvent,
        r#"
        SELECT id, short_code, scanned_at
        FROM scan_events
        WHERE short_code = $1
        ORDER BY scanned_at DESC
        "#,
        short_code
    )
    .fetch_all(pool)
    .await?;
    Ok(scans)
}