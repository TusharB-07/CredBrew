use crate::error::CoreError;
use crate::models::{Credential, CredentialStatus, TemplateKind};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// v1.0 Phase 3: Verifiable Credential persistence
// ─────────────────────────────────────────────────────────────────────────────

/// Persist the signed Verifiable Credential on a credentials row.
/// Called by the worker after `did::sign_document`. Also records the
/// status list allocation so Phase 4 revocation can flip the bit.
pub async fn set_credential_vc(
    pool: &PgPool,
    credential_id: Uuid,
    vc_id: &str,
    vc_json: &serde_json::Value,
    status_list_id: Uuid,
    status_list_index: i32,
) -> Result<(), CoreError> {
    sqlx::query(
        r#"
        UPDATE credentials
        SET vc_id = $2,
            vc_json = $3,
            status_list_id = $4,
            status_list_index = $5,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(credential_id)
    .bind(vc_id)
    .bind(vc_json)
    .bind(status_list_id)
    .bind(status_list_index)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_credentials_for_type(
    pool: &PgPool,
    participant_type_id: Uuid,
    template_id: Uuid,
    kind: TemplateKind,
) -> Result<Vec<Uuid>, CoreError> {
    let ids = sqlx::query_scalar!(
        r#"
        INSERT INTO credentials (participant_id, template_id, kind)
        SELECT p.id, $2, $3
        FROM participants p
        WHERE p.participant_type_id = $1
        ON CONFLICT (participant_id, template_id) DO NOTHING
        RETURNING id
        "#,
        participant_type_id,
        template_id,
        kind as TemplateKind
    )
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

pub async fn get_credential(pool: &PgPool, id: Uuid) -> Result<Option<Credential>, CoreError> {
    let credential = sqlx::query_as::<_, Credential>(
        r#"
        SELECT id, participant_id, template_id, kind as "kind", file_key,
               status as "status", attempts, last_error, issued_at, updated_at,
               vc_id, vc_json, status_list_id, status_list_index
        FROM credentials
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(credential)
}

pub async fn get_credentials_by_event(
    pool: &PgPool,
    event_id: Uuid,
    participant_type_id: Option<Uuid>,
    kind: Option<TemplateKind>,
) -> Result<Vec<Credential>, CoreError> {
    let mut query = String::from(
        r#"
        SELECT c.id, c.participant_id, c.template_id, c.kind as "kind", c.file_key,
               c.status as "status", c.attempts, c.last_error, c.issued_at, c.updated_at,
               c.vc_id, c.vc_json, c.status_list_id, c.status_list_index
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        WHERE p.event_id = $1
        "#,
    );
    let mut param_idx = 2;

    if participant_type_id.is_some() {
        query.push_str(&format!(" AND p.participant_type_id = ${}", param_idx));
        param_idx += 1;
    }
    if kind.is_some() {
        query.push_str(&format!(" AND c.kind = ${}", param_idx));
    }

    query.push_str(" ORDER BY c.issued_at DESC");

    let mut q = sqlx::query_as(&query).bind(event_id);

    if let Some(type_id) = participant_type_id {
        q = q.bind(type_id);
    }
    if let Some(k) = kind {
        q = q.bind(k as TemplateKind);
    }

    let credentials = q.fetch_all(pool).await?;
    Ok(credentials)
}

pub async fn get_credential_by_participant_and_template(
    pool: &PgPool,
    participant_id: Uuid,
    template_id: Uuid,
) -> Result<Option<Credential>, CoreError> {
    let credential = sqlx::query_as::<_, Credential>(
        r#"
        SELECT id, participant_id, template_id, kind as "kind", file_key,
               status as "status", attempts, last_error, issued_at, updated_at,
               vc_id, vc_json, status_list_id, status_list_index
        FROM credentials
        WHERE participant_id = $1 AND template_id = $2
        "#,
    )
    .bind(participant_id)
    .bind(template_id)
    .fetch_optional(pool)
    .await?;
    Ok(credential)
}

pub async fn update_credential_status(
    pool: &PgPool,
    id: Uuid,
    status: CredentialStatus,
    file_key: Option<&str>,
    last_error: Option<&str>,
) -> Result<Option<Credential>, CoreError> {
    let credential = sqlx::query_as::<_, Credential>(
        r#"
        UPDATE credentials
        SET status = $2,
            file_key = COALESCE($3, file_key),
            last_error = $4,
            attempts = CASE WHEN $2 = 'failed' THEN attempts + 1 ELSE attempts END,
            updated_at = now()
        WHERE id = $1
        RETURNING id, participant_id, template_id, kind as "kind", file_key,
                  status as "status", attempts, last_error, issued_at, updated_at,
                  vc_id, vc_json, status_list_id, status_list_index
        "#,
    )
    .bind(id)
    .bind(status as CredentialStatus)
    .bind(file_key)
    .bind(last_error)
    .fetch_optional(pool)
    .await?;
    Ok(credential)
}

pub async fn reset_credential_to_pending(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<Credential>, CoreError> {
    let credential = sqlx::query_as::<_, Credential>(
        r#"
        UPDATE credentials
        SET status = 'pending',
            last_error = NULL,
            updated_at = now()
        WHERE id = $1
        RETURNING id, participant_id, template_id, kind as "kind", file_key,
                  status as "status", attempts, last_error, issued_at, updated_at,
                  vc_id, vc_json, status_list_id, status_list_index
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(credential)
}

pub async fn revoke_credential(pool: &PgPool, id: Uuid) -> Result<Option<Credential>, CoreError> {
    let credential = sqlx::query_as::<_, Credential>(
        r#"
        UPDATE credentials
        SET status = 'revoked',
            updated_at = now()
        WHERE id = $1
        RETURNING id, participant_id, template_id, kind as "kind", file_key,
                  status as "status", attempts, last_error, issued_at, updated_at,
                  vc_id, vc_json, status_list_id, status_list_index
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(credential)
}

pub async fn get_issue_status(
    pool: &PgPool,
    event_id: Uuid,
    participant_type_id: Option<Uuid>,
    kind: Option<TemplateKind>,
) -> Result<(i64, i64, i64, i64, i64), CoreError> {
    let mut where_clause = String::from("WHERE p.event_id = $1");
    let mut param_idx = 2;

    if participant_type_id.is_some() {
        where_clause.push_str(&format!(" AND p.participant_type_id = ${}", param_idx));
        param_idx += 1;
    }
    if kind.is_some() {
        where_clause.push_str(&format!(" AND c.kind = ${}", param_idx));
    }

    let query = format!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE c.status = 'pending') as pending,
            COUNT(*) FILTER (WHERE c.status = 'rendered') as rendered,
            COUNT(*) FILTER (WHERE c.status = 'emailed') as emailed,
            COUNT(*) FILTER (WHERE c.status = 'failed') as failed,
            COUNT(*) as total
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        {}
        "#,
        where_clause
    );

    let mut q = sqlx::query(&query).bind(event_id);

    if let Some(type_id) = participant_type_id {
        q = q.bind(type_id);
    }
    if let Some(k) = kind {
        q = q.bind(k as TemplateKind);
    }

    let row = q.fetch_one(pool).await?;
    Ok((
        row.get::<i64, _>("pending"),
        row.get::<i64, _>("rendered"),
        row.get::<i64, _>("emailed"),
        row.get::<i64, _>("failed"),
        row.get::<i64, _>("total"),
    ))
}

pub async fn get_event_credential_counts(
    pool: &PgPool,
    event_id: Uuid,
) -> Result<(i64, i64), CoreError> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE c.status = 'emailed') as emailed,
            COUNT(*) FILTER (WHERE c.status = 'failed') as failed
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        WHERE p.event_id = $1
        "#,
    )
    .bind(event_id)
    .fetch_one(pool)
    .await?;
    Ok((row.get::<i64, _>("emailed"), row.get::<i64, _>("failed")))
}

pub async fn get_issued_counts_by_type(
    pool: &PgPool,
    event_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, (i64, i64)>, CoreError> {
    let rows = sqlx::query(
        r#"
        SELECT p.participant_type_id as pt_id,
               COUNT(*) FILTER (WHERE c.kind = 'badge') as badges,
               COUNT(*) FILTER (WHERE c.kind = 'certificate') as certs
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        WHERE p.event_id = $1
          AND c.status IN ('rendered', 'emailed')
        GROUP BY p.participant_type_id
        "#,
    )
    .bind(event_id)
    .fetch_all(pool)
    .await?;

    let mut map = std::collections::HashMap::new();
    for row in rows {
        let pt_id: Uuid = row.get("pt_id");
        let badges: i64 = row.get("badges");
        let certs: i64 = row.get("certs");
        map.insert(pt_id, (badges, certs));
    }
    Ok(map)
}

pub async fn get_credential_statuses_by_participants(
    pool: &PgPool,
    participant_ids: &[Uuid],
) -> Result<
    std::collections::HashMap<Uuid, (Option<CredentialStatus>, Option<CredentialStatus>)>,
    CoreError,
> {
    let mut map = std::collections::HashMap::new();
    if participant_ids.is_empty() {
        return Ok(map);
    }

    let query = r#"
        SELECT participant_id,
               kind as "kind",
               status as "status"
        FROM credentials
        WHERE participant_id = ANY($1)
    "#;

    let rows = sqlx::query_as::<_, (Uuid, TemplateKind, CredentialStatus)>(query)
        .bind(participant_ids)
        .fetch_all(pool)
        .await?;

    for (participant_id, kind, status) in rows {
        let entry = map.entry(participant_id).or_insert((None, None));
        match kind {
            TemplateKind::Badge => entry.0 = Some(status),
            TemplateKind::Certificate => entry.1 = Some(status),
        }
    }
    Ok(map)
}

pub async fn get_recent_failures(
    pool: &PgPool,
    event_id: Uuid,
    limit: i64,
) -> Result<Vec<Credential>, CoreError> {
    let credentials = sqlx::query_as::<_, Credential>(
        r#"
        SELECT c.id, c.participant_id, c.template_id, c.kind as "kind", c.file_key,
               c.status as "status", c.attempts, c.last_error, c.issued_at, c.updated_at,
               c.vc_id, c.vc_json, c.status_list_id, c.status_list_index
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        WHERE p.event_id = $1 AND c.status = 'failed'
        ORDER BY c.updated_at DESC
        LIMIT $2
        "#,
    )
    .bind(event_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(credentials)
}

pub async fn get_credential_with_details(
    pool: &PgPool,
    credential_id: Uuid,
) -> Result<
    Option<(
        Credential,
        crate::models::Participant,
        crate::models::ParticipantType,
        crate::models::Event,
        Template,
    )>,
    CoreError,
> {
    let row = sqlx::query(
        r#"
        SELECT
            c.id, c.participant_id, c.template_id, c.kind, c.file_key,
            c.status, c.attempts, c.last_error, c.issued_at, c.updated_at,
            c.vc_id, c.vc_json, c.status_list_id, c.status_list_index,
            p.id as p_id, p.event_id, p.participant_type_id, p.name as p_name, p.email, p.short_code, p.created_at as p_created_at,
            pt.id as pt_id, pt.event_id as pt_event_id, pt.name as pt_name, pt.badge_enabled, pt.certificate_enabled,
            e.id as e_id, e.owner_id, e.name as e_name, e.event_date, e.org_name, e.accent_color, e.created_at as e_created_at,
            t.id as t_id, t.participant_type_id as t_pt_id, t.kind, t.file_key as t_file_key, t.name_zone, t.qr_zone
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        JOIN participant_types pt ON p.participant_type_id = pt.id
        JOIN events e ON p.event_id = e.id
        JOIN templates t ON c.template_id = t.id
        WHERE c.id = $1
        "#,
    )
    .bind(credential_id)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let credential = Credential {
            id: row.get("id"),
            participant_id: row.get("participant_id"),
            template_id: row.get("template_id"),
            kind: row.get::<TemplateKind, _>("kind"),
            file_key: row.get("file_key"),
            status: row.get::<CredentialStatus, _>("status"),
            attempts: row.get("attempts"),
            last_error: row.get("last_error"),
            issued_at: row.get("issued_at"),
            updated_at: row.get("updated_at"),
            vc_id: row.get("vc_id"),
            vc_json: row.get("vc_json"),
            status_list_id: row.get("status_list_id"),
            status_list_index: row.get("status_list_index"),
        };
        let participant = crate::models::Participant {
            id: row.get("p_id"),
            event_id: row.get("event_id"),
            participant_type_id: row.get("participant_type_id"),
            name: row.get("p_name"),
            email: row.get("email"),
            short_code: row.get("short_code"),
            created_at: row.get("p_created_at"),
        };
        let participant_type = crate::models::ParticipantType {
            id: row.get("pt_id"),
            event_id: row.get("pt_event_id"),
            name: row.get("pt_name"),
            badge_enabled: row.get("badge_enabled"),
            certificate_enabled: row.get("certificate_enabled"),
        };
        let event = crate::models::Event {
            id: row.get("e_id"),
            owner_id: row.get("owner_id"),
            tenant_id: None, // v1.0: not needed by the worker; tenant scoping is enforced at the route layer.
            name: row.get("e_name"),
            event_date: row.get("event_date"),
            org_name: row.get("org_name"),
            accent_color: row.get("accent_color"),
            created_at: row.get("e_created_at"),
        };
        let template = Template {
            id: row.get("t_id"),
            participant_type_id: row.get("t_pt_id"),
            kind: row.get::<TemplateKind, _>("t_kind"),
            file_key: row.get("t_file_key"),
            name_zone: row.get("name_zone"),
            qr_zone: row.get("qr_zone"),
        };
        Ok(Some((
            credential,
            participant,
            participant_type,
            event,
            template,
        )))
    } else {
        Ok(None)
    }
}

pub async fn get_credential_by_short_code(
    pool: &PgPool,
    short_code: &str,
) -> Result<
    Option<(
        Credential,
        crate::models::Participant,
        crate::models::ParticipantType,
        crate::models::Event,
        Template,
    )>,
    CoreError,
> {
    let row = sqlx::query(
        r#"
        SELECT
            c.id, c.participant_id, c.template_id, c.kind, c.file_key,
            c.status, c.attempts, c.last_error, c.issued_at, c.updated_at,
            c.vc_id, c.vc_json, c.status_list_id, c.status_list_index,
            p.id as p_id, p.event_id, p.participant_type_id, p.name as p_name, p.email, p.short_code, p.created_at as p_created_at,
            pt.id as pt_id, pt.event_id as pt_event_id, pt.name as pt_name, pt.badge_enabled, pt.certificate_enabled,
            e.id as e_id, e.owner_id, e.name as e_name, e.event_date, e.org_name, e.accent_color, e.created_at as e_created_at,
            t.id as t_id, t.participant_type_id as t_pt_id, t.kind, t.file_key as t_file_key, t.name_zone, t.qr_zone
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        JOIN participant_types pt ON p.participant_type_id = pt.id
        JOIN events e ON p.event_id = e.id
        JOIN templates t ON c.template_id = t.id
        WHERE p.short_code = $1
        "#,
    )
    .bind(short_code)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let credential = Credential {
            id: row.get("id"),
            participant_id: row.get("participant_id"),
            template_id: row.get("template_id"),
            kind: row.get::<TemplateKind, _>("kind"),
            file_key: row.get("file_key"),
            status: row.get::<CredentialStatus, _>("status"),
            attempts: row.get("attempts"),
            last_error: row.get("last_error"),
            issued_at: row.get("issued_at"),
            updated_at: row.get("updated_at"),
            vc_id: row.get("vc_id"),
            vc_json: row.get("vc_json"),
            status_list_id: row.get("status_list_id"),
            status_list_index: row.get("status_list_index"),
        };
        let participant = crate::models::Participant {
            id: row.get("p_id"),
            event_id: row.get("event_id"),
            participant_type_id: row.get("participant_type_id"),
            name: row.get("p_name"),
            email: row.get("email"),
            short_code: row.get("short_code"),
            created_at: row.get("p_created_at"),
        };
        let participant_type = crate::models::ParticipantType {
            id: row.get("pt_id"),
            event_id: row.get("pt_event_id"),
            name: row.get("pt_name"),
            badge_enabled: row.get("badge_enabled"),
            certificate_enabled: row.get("certificate_enabled"),
        };
        let event = crate::models::Event {
            id: row.get("e_id"),
            owner_id: row.get("owner_id"),
            tenant_id: None, // v1.0: not needed by the verify flow; tenant scoping is enforced at the route layer.
            name: row.get("e_name"),
            event_date: row.get("event_date"),
            org_name: row.get("org_name"),
            accent_color: row.get("accent_color"),
            created_at: row.get("e_created_at"),
        };
        let template = Template {
            id: row.get("t_id"),
            participant_type_id: row.get("t_pt_id"),
            kind: row.get::<TemplateKind, _>("t_kind"),
            file_key: row.get("t_file_key"),
            name_zone: row.get("name_zone"),
            qr_zone: row.get("qr_zone"),
        };
        Ok(Some((
            credential,
            participant,
            participant_type,
            event,
            template,
        )))
    } else {
        Ok(None)
    }
}

use crate::models::Template;
