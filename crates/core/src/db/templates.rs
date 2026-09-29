use sqlx::PgPool;
use uuid::Uuid;
use crate::models::{Template, TemplateKind};
use crate::error::CoreError;

pub async fn create_template(
    pool: &PgPool,
    participant_type_id: Uuid,
    kind: TemplateKind,
    file_key: &str,
    name_zone: serde_json::Value,
    qr_zone: serde_json::Value,
) -> Result<Template, CoreError> {
    let template = sqlx::query_as!(
        Template,
        r#"
        INSERT INTO templates (participant_type_id, kind, file_key, name_zone, qr_zone)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (participant_type_id, kind) DO UPDATE SET
            file_key = EXCLUDED.file_key,
            name_zone = EXCLUDED.name_zone,
            qr_zone = EXCLUDED.qr_zone
        RETURNING id, participant_type_id, kind as "kind: TemplateKind", file_key, name_zone, qr_zone
        "#,
        participant_type_id, kind as TemplateKind, file_key, name_zone, qr_zone
    )
    .fetch_one(pool)
    .await?;
    Ok(template)
}

pub async fn get_template(pool: &PgPool, id: Uuid) -> Result<Option<Template>, CoreError> {
    let template = sqlx::query_as!(
        Template,
        r#"
        SELECT id, participant_type_id, kind as "kind: TemplateKind", file_key, name_zone, qr_zone
        FROM templates
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(template)
}

pub async fn get_templates_by_type(pool: &PgPool, participant_type_id: Uuid) -> Result<Vec<Template>, CoreError> {
    let templates = sqlx::query_as!(
        Template,
        r#"
        SELECT id, participant_type_id, kind as "kind: TemplateKind", file_key, name_zone, qr_zone
        FROM templates
        WHERE participant_type_id = $1
        "#,
        participant_type_id
    )
    .fetch_all(pool)
    .await?;
    Ok(templates)
}

pub async fn get_template_by_type_and_kind(
    pool: &PgPool,
    participant_type_id: Uuid,
    kind: TemplateKind,
) -> Result<Option<Template>, CoreError> {
    let template = sqlx::query_as!(
        Template,
        r#"
        SELECT id, participant_type_id, kind as "kind: TemplateKind", file_key, name_zone, qr_zone
        FROM templates
        WHERE participant_type_id = $1 AND kind = $2
        "#,
        participant_type_id, kind as TemplateKind
    )
    .fetch_optional(pool)
    .await?;
    Ok(template)
}

pub async fn delete_template(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
    let result = sqlx::query!(
        "DELETE FROM templates WHERE id = $1",
        id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_template_by_file_key(
    pool: &PgPool,
    file_key: &str,
) -> Result<Option<Template>, CoreError> {
    let template = sqlx::query_as!(
        Template,
        r#"
        SELECT id, participant_type_id, kind as "kind: TemplateKind", file_key, name_zone, qr_zone
        FROM templates
        WHERE file_key = $1
        "#,
        file_key
    )
    .fetch_optional(pool)
    .await?;
    Ok(template)
}

pub async fn get_template_kinds_by_event(
    pool: &PgPool,
    event_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, std::collections::HashSet<TemplateKind>>, CoreError> {
    use sqlx::Row;

    let rows = sqlx::query(
        r#"
        SELECT pt.id as type_id, t.kind as "kind"
        FROM templates t
        JOIN participant_types pt ON t.participant_type_id = pt.id
        WHERE pt.event_id = $1
        "#,
    )
    .bind(event_id)
    .fetch_all(pool)
    .await?;

    let mut map: std::collections::HashMap<Uuid, std::collections::HashSet<TemplateKind>> =
        std::collections::HashMap::new();
    for row in rows {
        let type_id: Uuid = row.get("type_id");
        let kind: TemplateKind = row.get("kind");
        map.entry(type_id).or_default().insert(kind);
    }
    Ok(map)
}