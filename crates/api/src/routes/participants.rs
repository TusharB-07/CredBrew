use axum::{
    extract::{State, Path, Query, Multipart},
    Json,
};
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use csv::ReaderBuilder;
use regex::Regex;
use once_cell::sync::Lazy;
use crate::state::AppState;
use crate::routes::auth::AuthUser;
use credbrew_core::{
    db::{credentials, participants, participant_types, events},
    models::{CredentialStatus, Participant},
    short_code::generate_unique_short_code,
    error::CoreError,
};

static EMAIL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[^@\s]+@[^@\s]+\.[^@\s]+$").unwrap());

#[derive(Deserialize)]
pub struct CreateParticipantRequest {
    pub name: String,
    pub email: String,
    pub participant_type_id: Uuid,
}

#[derive(Serialize)]
pub struct ParticipantWithCredentialStatus {
    #[serde(flatten)]
    pub participant: Participant,
    pub badge_status: Option<String>,
    pub certificate_status: Option<String>,
}

#[derive(Serialize)]
pub struct PaginatedParticipants {
    pub participants: Vec<ParticipantWithCredentialStatus>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub imported: usize,
    pub errors: Vec<ImportError>,
}

#[derive(Serialize)]
pub struct ImportError {
    pub row: usize,
    pub field: String,
    pub message: String,
}

#[axum::debug_handler]
pub async fn create_participant(
    State(state): State<std::sync::Arc<AppState>>,
    Path(event_id): Path<Uuid>,
    user: AuthUser,
    Json(payload): Json<CreateParticipantRequest>,
) -> Result<Json<Participant>, CoreError> {
    let event = events::get_event(&state.pool, event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let pt = participant_types::get_participant_type(&state.pool, payload.participant_type_id).await?
        .ok_or_else(|| CoreError::NotFound("participant type not found".into()))?;

    if pt.event_id != event_id {
        return Err(CoreError::Forbidden("participant type does not belong to event".into()));
    }

    let existing = participants::get_participant_by_event_and_email(&state.pool, event_id, &payload.email).await?;
    if existing.is_some() {
        return Err(CoreError::Conflict("email already registered for this event".into()));
    }

    let short_code = generate_unique_short_code(|code| {
        let pool = state.pool.clone();
        async move {
            let existing = sqlx::query_scalar!("SELECT 1 FROM participants WHERE short_code = $1", code)
                .fetch_optional(&pool)
                .await?;
            Ok(existing.is_some())
        }
    }).await?;

    let participant = participants::create_participant(
        &state.pool,
        event_id,
        payload.participant_type_id,
        &payload.name,
        &payload.email,
        &short_code,
    ).await?;

    Ok(Json(participant))
}

#[axum::debug_handler]
pub async fn list_participants(
    State(state): State<std::sync::Arc<AppState>>,
    Path(event_id): Path<Uuid>,
    Query(params): Query<ListParams>,
    user: AuthUser,
) -> Result<Json<PaginatedParticipants>, CoreError> {
    let event = events::get_event(&state.pool, event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).clamp(1, 200);

    let participants_list = participants::get_participants_by_event(
        &state.pool,
        event_id,
        params.type_id,
        page,
        per_page,
    ).await?;

    let total = participants::count_participants_by_event(&state.pool, event_id, params.type_id).await?;

    let ids: Vec<Uuid> = participants_list.iter().map(|p| p.id).collect();
    let statuses = credentials::get_credential_statuses_by_participants(&state.pool, &ids).await?;

    let mut result = Vec::new();
    for p in participants_list {
        let (badge_status, certificate_status) = statuses.get(&p.id).copied().unwrap_or((None, None));
        result.push(ParticipantWithCredentialStatus {
            participant: p,
            badge_status: badge_status.map(status_lower),
            certificate_status: certificate_status.map(status_lower),
        });
    }

    Ok(Json(PaginatedParticipants {
        participants: result,
        total,
        page,
        per_page,
    }))
}

fn status_lower(status: CredentialStatus) -> String {
    format!("{:?}", status).to_lowercase()
}

#[derive(Deserialize)]
pub struct ListParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub type_id: Option<Uuid>,
}

#[axum::debug_handler]
pub async fn import_participants(
    State(state): State<std::sync::Arc<AppState>>,
    Path(event_id): Path<Uuid>,
    user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<ImportResult>, CoreError> {
    let event = events::get_event(&state.pool, event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let types = participant_types::get_participant_types_by_event(&state.pool, event_id).await?;
    let type_map: std::collections::HashMap<String, Uuid> = types
        .iter()
        .map(|t| (t.name.to_lowercase(), t.id))
        .collect();

    let mut csv_data = None;
    while let Some(field) = multipart.next_field().await? {
        if field.name() == Some("file") {
            let data = field.bytes().await?;
            csv_data = Some(data);
            break;
        }
    }

    let csv_data = csv_data.ok_or_else(|| CoreError::TemplateValidation("no file provided".into()))?;
    if csv_data.len() > 5_000 * 200 {
        return Err(CoreError::TemplateValidation("file too large (max ~5000 rows)".into()));
    }

    let mut rdr = ReaderBuilder::new().from_reader(&csv_data[..]);
    let mut imported = 0;
    let mut errors = Vec::new();
    let mut seen_emails = std::collections::HashSet::new();

    for (row_idx, record) in rdr.deserialize::<CsvRow>().enumerate() {
        let row_num = row_idx + 2;
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                errors.push(ImportError { row: row_num, field: "csv".into(), message: format!("parse error: {}", e) });
                continue;
            }
        };

        if record.name.trim().is_empty() {
            errors.push(ImportError { row: row_num, field: "name".into(), message: "name is required".into() });
            continue;
        }

        if !email_validator(&record.email) {
            errors.push(ImportError { row: row_num, field: "email".into(), message: "invalid email format".into() });
            continue;
        }

        let email_lower = record.email.to_lowercase();
        if seen_emails.contains(&email_lower) {
            errors.push(ImportError { row: row_num, field: "email".into(), message: "duplicate email in file".into() });
            continue;
        }

        let existing = participants::get_participant_by_event_and_email(&state.pool, event_id, &record.email).await?;
        if existing.is_some() {
            errors.push(ImportError { row: row_num, field: "email".into(), message: "email already registered for this event".into() });
            continue;
        }

        let type_id = match type_map.get(&record.type_name.to_lowercase()) {
            Some(id) => *id,
            None => {
                errors.push(ImportError { row: row_num, field: "type".into(), message: "unknown participant type".into() });
                continue;
            }
        };

        seen_emails.insert(email_lower);

        let short_code = generate_unique_short_code(|code| {
            let pool = state.pool.clone();
            async move {
                let existing = sqlx::query_scalar!("SELECT 1 FROM participants WHERE short_code = $1", code)
                    .fetch_optional(&pool)
                    .await?;
                Ok(existing.is_some())
            }
        }).await?;

        match participants::create_participant(
            &state.pool,
            event_id,
            type_id,
            &record.name,
            &record.email,
            &short_code,
        ).await {
            Ok(_) => imported += 1,
            Err(e) => errors.push(ImportError { row: row_num, field: "db".into(), message: e.to_string() }),
        }
    }

    Ok(Json(ImportResult { imported, errors }))
}

#[derive(Deserialize)]
struct CsvRow {
    name: String,
    email: String,
    #[serde(rename = "type")]
    type_name: String,
}

fn email_validator(email: &str) -> bool {
    EMAIL_RE.is_match(email)
}

#[axum::debug_handler]
pub async fn delete_participant(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
) -> Result<(), CoreError> {
    let participant = participants::get_participant_by_id(&state.pool, id).await?
        .ok_or_else(|| CoreError::NotFound("participant not found".into()))?;

    let event = events::get_event(&state.pool, participant.event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let has_final_creds = participants::has_emailed_or_revoked_credentials(&state.pool, id).await?;
    if has_final_creds {
        return Err(CoreError::Conflict("cannot delete participant with emailed or revoked credentials".into()));
    }

    participants::delete_participant(&state.pool, id).await?;
    Ok(())
}