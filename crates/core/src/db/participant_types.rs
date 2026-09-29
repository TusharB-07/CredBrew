use sqlx::PgPool;
use uuid::Uuid;
use crate::models::ParticipantType;
use crate::error::CoreError;

pub async fn create_participant_type(
    pool: &PgPool,
    event_id: Uuid,
    name: &str,
    badge_enabled: bool,
    certificate_enabled: bool,
) -> Result<ParticipantType, CoreError> {
    let pt = sqlx::query_as!(
        ParticipantType,
        r#"
        INSERT INTO participant_types (event_id, name, badge_enabled, certificate_enabled)
        VALUES ($1, $2, $3, $4)
        RETURNING id, event_id, name, badge_enabled, certificate_enabled
        "#,
        event_id, name, badge_enabled, certificate_enabled
    )
    .fetch_one(pool)
    .await?;
    Ok(pt)
}

pub async fn get_participant_types_by_event(pool: &PgPool, event_id: Uuid) -> Result<Vec<ParticipantType>, CoreError> {
    let types = sqlx::query_as!(
        ParticipantType,
        r#"
        SELECT id, event_id, name, badge_enabled, certificate_enabled
        FROM participant_types
        WHERE event_id = $1
        ORDER BY name
        "#,
        event_id
    )
    .fetch_all(pool)
    .await?;
    Ok(types)
}

pub async fn get_participant_type(pool: &PgPool, id: Uuid) -> Result<Option<ParticipantType>, CoreError> {
    let pt = sqlx::query_as!(
        ParticipantType,
        r#"
        SELECT id, event_id, name, badge_enabled, certificate_enabled
        FROM participant_types
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(pt)
}

pub async fn update_participant_type(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    badge_enabled: Option<bool>,
    certificate_enabled: Option<bool>,
) -> Result<Option<ParticipantType>, CoreError> {
    let pt = sqlx::query_as!(
        ParticipantType,
        r#"
        UPDATE participant_types
        SET name = COALESCE($2, name),
            badge_enabled = COALESCE($3, badge_enabled),
            certificate_enabled = COALESCE($4, certificate_enabled)
        WHERE id = $1
        RETURNING id, event_id, name, badge_enabled, certificate_enabled
        "#,
        id, name, badge_enabled, certificate_enabled
    )
    .fetch_optional(pool)
    .await?;
    Ok(pt)
}

pub async fn delete_participant_type(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
    let result = sqlx::query!(
        "DELETE FROM participant_types WHERE id = $1",
        id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_credentials_for_type(pool: &PgPool, participant_type_id: Uuid) -> Result<bool, CoreError> {
    let count: Option<i64> = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        WHERE p.participant_type_id = $1
        "#,
        participant_type_id
    )
    .fetch_one(pool)
    .await?;
    Ok(count.unwrap_or(0) > 0)
}

pub async fn create_default_types(pool: &PgPool, event_id: Uuid) -> Result<(), CoreError> {
    let defaults = [
        ("Speaker", true, true),
        ("Winner", true, true),
        ("Attendee", true, false),
    ];
    for (name, badge, cert) in defaults {
        sqlx::query!(
            r#"
            INSERT INTO participant_types (event_id, name, badge_enabled, certificate_enabled)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (event_id, name) DO NOTHING
            "#,
            event_id, name, badge, cert
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}