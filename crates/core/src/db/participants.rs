use sqlx::PgPool;
use uuid::Uuid;
use crate::models::Participant;
use crate::error::CoreError;

pub async fn create_participant(
    pool: &PgPool,
    event_id: Uuid,
    participant_type_id: Uuid,
    name: &str,
    email: &str,
    short_code: &str,
) -> Result<Participant, CoreError> {
    let participant = sqlx::query_as!(
        Participant,
        r#"
        INSERT INTO participants (event_id, participant_type_id, name, email, short_code)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, event_id, participant_type_id, name, email, short_code, created_at
        "#,
        event_id, participant_type_id, name, email, short_code
    )
    .fetch_one(pool)
    .await?;
    Ok(participant)
}

pub async fn get_participants_by_event(
    pool: &PgPool,
    event_id: Uuid,
    participant_type_id: Option<Uuid>,
    page: i64,
    per_page: i64,
) -> Result<Vec<Participant>, CoreError> {
    let offset = (page - 1) * per_page;
    let participants = if let Some(type_id) = participant_type_id {
        sqlx::query_as!(
            Participant,
            r#"
            SELECT id, event_id, participant_type_id, name, email, short_code, created_at
            FROM participants
            WHERE event_id = $1 AND participant_type_id = $2
            ORDER BY created_at
            LIMIT $3 OFFSET $4
            "#,
            event_id, type_id, per_page, offset
        )
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as!(
            Participant,
            r#"
            SELECT id, event_id, participant_type_id, name, email, short_code, created_at
            FROM participants
            WHERE event_id = $1
            ORDER BY created_at
            LIMIT $2 OFFSET $3
            "#,
            event_id, per_page, offset
        )
        .fetch_all(pool)
        .await?
    };
    Ok(participants)
}

pub async fn count_participants_by_event(
    pool: &PgPool,
    event_id: Uuid,
    participant_type_id: Option<Uuid>,
) -> Result<i64, CoreError> {
    let count = if let Some(type_id) = participant_type_id {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM participants WHERE event_id = $1 AND participant_type_id = $2",
            event_id, type_id
        )
        .fetch_one(pool)
        .await?
    } else {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM participants WHERE event_id = $1",
            event_id
        )
        .fetch_one(pool)
        .await?
    };
    Ok(count.unwrap_or(0))
}

pub async fn get_participant_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Participant>, CoreError> {
    let participant = sqlx::query_as!(
        Participant,
        r#"
        SELECT id, event_id, participant_type_id, name, email, short_code, created_at
        FROM participants
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(participant)
}

pub async fn get_participant_by_event_and_email(
    pool: &PgPool,
    event_id: Uuid,
    email: &str,
) -> Result<Option<Participant>, CoreError> {
    let participant = sqlx::query_as!(
        Participant,
        r#"
        SELECT id, event_id, participant_type_id, name, email, short_code, created_at
        FROM participants
        WHERE event_id = $1 AND email = $2
        "#,
        event_id, email
    )
    .fetch_optional(pool)
    .await?;
    Ok(participant)
}

pub async fn delete_participant(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
    let result = sqlx::query!(
        "DELETE FROM participants WHERE id = $1",
        id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_emailed_or_revoked_credentials(pool: &PgPool, participant_id: Uuid) -> Result<bool, CoreError> {
    let count: Option<i64> = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) FROM credentials
        WHERE participant_id = $1 AND status IN ('emailed', 'revoked')
        "#,
        participant_id
    )
    .fetch_one(pool)
    .await?;
    Ok(count.unwrap_or(0) > 0)
}