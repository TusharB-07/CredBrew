use sqlx::PgPool;
use uuid::Uuid;
use crate::models::Event;
use crate::error::CoreError;

pub async fn create_event(
    pool: &PgPool,
    owner_id: Uuid,
    name: &str,
    event_date: Option<chrono::NaiveDate>,
    org_name: &str,
    accent_color: &str,
) -> Result<Event, CoreError> {
    let event = sqlx::query_as!(
        Event,
        r#"
        INSERT INTO events (owner_id, name, event_date, org_name, accent_color)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, owner_id, name, event_date, org_name, accent_color, created_at
        "#,
        owner_id, name, event_date, org_name, accent_color
    )
    .fetch_one(pool)
    .await?;
    Ok(event)
}

pub async fn get_event(pool: &PgPool, id: Uuid) -> Result<Option<Event>, CoreError> {
    let event = sqlx::query_as!(
        Event,
        r#"
        SELECT id, owner_id, name, event_date, org_name, accent_color, created_at
        FROM events
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(event)
}

pub async fn get_events_by_owner(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Event>, CoreError> {
    let events = sqlx::query_as!(
        Event,
        r#"
        SELECT id, owner_id, name, event_date, org_name, accent_color, created_at
        FROM events
        WHERE owner_id = $1
        ORDER BY created_at DESC
        "#,
        owner_id
    )
    .fetch_all(pool)
    .await?;
    Ok(events)
}

pub async fn update_event(
    pool: &PgPool,
    id: Uuid,
    owner_id: Uuid,
    name: Option<&str>,
    event_date: Option<chrono::NaiveDate>,
    org_name: Option<&str>,
    accent_color: Option<&str>,
) -> Result<Option<Event>, CoreError> {
    let event = sqlx::query_as!(
        Event,
        r#"
        UPDATE events
        SET name = COALESCE($3, name),
            event_date = COALESCE($4, event_date),
            org_name = COALESCE($5, org_name),
            accent_color = COALESCE($6, accent_color)
        WHERE id = $1 AND owner_id = $2
        RETURNING id, owner_id, name, event_date, org_name, accent_color, created_at
        "#,
        id, owner_id, name, event_date, org_name, accent_color
    )
    .fetch_optional(pool)
    .await?;
    Ok(event)
}

pub async fn delete_event(pool: &PgPool, id: Uuid, owner_id: Uuid) -> Result<bool, CoreError> {
    let result = sqlx::query!(
        "DELETE FROM events WHERE id = $1 AND owner_id = $2",
        id, owner_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}