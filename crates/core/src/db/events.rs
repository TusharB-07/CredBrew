use crate::error::CoreError;
use crate::models::Event;
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Event queries (v1.0 — tenant-scoped)
//
// All functions now take a `tenant_id` parameter and filter by it. This is the
// pattern the other entity modules (participant_types, participants, templates,
// credentials) will follow in subsequent PRs.
//
// Uses runtime-checked `sqlx::query_as` instead of `query_as!` because the
// Event struct now has a `tenant_id` field and the .sqlx offline cache would
// need regeneration against a live DB to use the compile-time macros.
// ─────────────────────────────────────────────────────────────────────────────

const EVENT_COLUMNS: &str =
    "id, owner_id, tenant_id, name, event_date, org_name, accent_color, created_at";

pub async fn create_event(
    pool: &PgPool,
    owner_id: Uuid,
    tenant_id: Uuid,
    name: &str,
    event_date: Option<NaiveDate>,
    org_name: &str,
    accent_color: &str,
) -> Result<Event, CoreError> {
    let event = sqlx::query_as::<_, Event>(&format!(
        r#"
        INSERT INTO events (owner_id, tenant_id, name, event_date, org_name, accent_color)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING {EVENT_COLUMNS}
        "#
    ))
    .bind(owner_id)
    .bind(tenant_id)
    .bind(name)
    .bind(event_date)
    .bind(org_name)
    .bind(accent_color)
    .fetch_one(pool)
    .await?;
    Ok(event)
}

/// Fetch an event by id, scoped to a tenant. Returns None if the event does
/// not exist OR exists but belongs to a different tenant — by design, so
/// cross-tenant access returns 404 (not 403, to prevent tenant-id enumeration).
pub async fn get_event_for_tenant(
    pool: &PgPool,
    id: Uuid,
    tenant_id: Uuid,
) -> Result<Option<Event>, CoreError> {
    let event = sqlx::query_as::<_, Event>(&format!(
        r#"
        SELECT {EVENT_COLUMNS}
        FROM events
        WHERE id = $1 AND tenant_id = $2
        "#
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(event)
}

/// List events for a tenant, newest first.
pub async fn list_events_for_tenant(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Vec<Event>, CoreError> {
    let events = sqlx::query_as::<_, Event>(&format!(
        r#"
        SELECT {EVENT_COLUMNS}
        FROM events
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        "#
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    Ok(events)
}

/// Update an event, scoped to a tenant. Returns None if the event does not
/// exist or belongs to a different tenant.
pub async fn update_event_for_tenant(
    pool: &PgPool,
    id: Uuid,
    tenant_id: Uuid,
    name: Option<&str>,
    event_date: Option<NaiveDate>,
    org_name: Option<&str>,
    accent_color: Option<&str>,
) -> Result<Option<Event>, CoreError> {
    let event = sqlx::query_as::<_, Event>(&format!(
        r#"
        UPDATE events
        SET name = COALESCE($3, name),
            event_date = COALESCE($4, event_date),
            org_name = COALESCE($5, org_name),
            accent_color = COALESCE($6, accent_color)
        WHERE id = $1 AND tenant_id = $2
        RETURNING {EVENT_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(tenant_id)
    .bind(name)
    .bind(event_date)
    .bind(org_name)
    .bind(accent_color)
    .fetch_optional(pool)
    .await?;
    Ok(event)
}

/// Delete an event, scoped to a tenant.
pub async fn delete_event_for_tenant(
    pool: &PgPool,
    id: Uuid,
    tenant_id: Uuid,
) -> Result<bool, CoreError> {
    let result = sqlx::query("DELETE FROM events WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Legacy functions (deprecated, retained for the backfill binary and any
// unmigrated callers). New code should use the `_for_tenant` variants above.
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_event(pool: &PgPool, id: Uuid) -> Result<Option<Event>, CoreError> {
    let event = sqlx::query_as::<_, Event>(&format!(
        r#"
        SELECT {EVENT_COLUMNS}
        FROM events
        WHERE id = $1
        "#
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(event)
}

pub async fn get_events_by_owner(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Event>, CoreError> {
    let events = sqlx::query_as::<_, Event>(&format!(
        r#"
        SELECT {EVENT_COLUMNS}
        FROM events
        WHERE owner_id = $1
        ORDER BY created_at DESC
        "#
    ))
    .bind(owner_id)
    .fetch_all(pool)
    .await?;
    Ok(events)
}

/// Set tenant_id on all events owned by a given user. Used by the backfill
/// binary to migrate v0.1 events into the user's new default tenant.
pub async fn assign_tenant_to_owner_events(
    pool: &PgPool,
    owner_id: Uuid,
    tenant_id: Uuid,
) -> Result<u64, CoreError> {
    let result =
        sqlx::query("UPDATE events SET tenant_id = $2 WHERE owner_id = $1 AND tenant_id IS NULL")
            .bind(owner_id)
            .bind(tenant_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}
