use axum::{
    extract::{State, Path},
    Json,
};
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use crate::state::AppState;
use crate::routes::auth::AuthUser;
use credbrew_core::{
    db::{credentials, events, participant_types, participants, templates},
    models::{Event, ParticipantType, TemplateKind},
    error::CoreError,
};

#[derive(Deserialize)]
pub struct CreateEventRequest {
    pub name: String,
    pub event_date: Option<chrono::NaiveDate>,
    pub org_name: Option<String>,
    pub accent_color: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateEventRequest {
    pub name: Option<String>,
    pub event_date: Option<chrono::NaiveDate>,
    pub org_name: Option<String>,
    pub accent_color: Option<String>,
}

#[derive(Serialize)]
pub struct EventWithStats {
    #[serde(flatten)]
    pub event: Event,
    pub participant_types: Vec<ParticipantTypeWithCounts>,
    pub total_participants: i64,
    pub credentials_emailed: i64,
    pub credentials_failed: i64,
}

#[derive(Serialize)]
pub struct ParticipantTypeWithCounts {
    #[serde(flatten)]
    pub participant_type: ParticipantType,
    pub participant_count: i64,
    pub badge_template: bool,
    pub certificate_template: bool,
    pub badges_issued: i64,
    pub certificates_issued: i64,
}

#[axum::debug_handler]
pub async fn create_event(
    State(state): State<std::sync::Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<CreateEventRequest>,
) -> Result<Json<Event>, CoreError> {
    let event = events::create_event(
        &state.pool,
        user.id,
        &payload.name,
        payload.event_date,
        payload.org_name.as_deref().unwrap_or(""),
        payload.accent_color.as_deref().unwrap_or("#4F46E5"),
    ).await?;

    participant_types::create_default_types(&state.pool, event.id).await?;

    Ok(Json(event))
}

#[axum::debug_handler]
pub async fn list_events(
    State(state): State<std::sync::Arc<AppState>>,
    user: AuthUser,
) -> Result<Json<Vec<EventWithStats>>, CoreError> {
    let events = events::get_events_by_owner(&state.pool, user.id).await?;

    let mut result = Vec::new();
    for event in events {
        result.push(get_event_stats(&state, &event).await?);
    }

    Ok(Json(result))
}

async fn get_event_stats(
    state: &std::sync::Arc<AppState>,
    event: &Event,
) -> Result<EventWithStats, CoreError> {
    let types = participant_types::get_participant_types_by_event(&state.pool, event.id).await?;

    let total_participants = participants::count_participants_by_event(&state.pool, event.id, None)
        .await?;
    let (credentials_emailed, credentials_failed) =
        credentials::get_event_credential_counts(&state.pool, event.id).await?;
    let issued_by_type = credentials::get_issued_counts_by_type(&state.pool, event.id).await?;
    let template_kinds = templates::get_template_kinds_by_event(&state.pool, event.id).await?;

    let mut type_details = Vec::new();
    for pt in types {
        let count =
            participants::count_participants_by_event(&state.pool, event.id, Some(pt.id)).await?;
        let kinds = template_kinds.get(&pt.id);
        let (badges_issued, certificates_issued) =
            issued_by_type.get(&pt.id).copied().unwrap_or((0, 0));
        type_details.push(ParticipantTypeWithCounts {
            participant_type: pt,
            participant_count: count,
            badge_template: kinds.is_some_and(|k| k.contains(&TemplateKind::Badge)),
            certificate_template: kinds.is_some_and(|k| k.contains(&TemplateKind::Certificate)),
            badges_issued,
            certificates_issued,
        });
    }

    Ok(EventWithStats {
        event: event.clone(),
        participant_types: type_details,
        total_participants,
        credentials_emailed,
        credentials_failed,
    })
}

#[axum::debug_handler]
pub async fn get_event(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
) -> Result<Json<EventWithStats>, CoreError> {
    let event = events::get_event(&state.pool, id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let stats = get_event_stats(&state, &event).await?;

    Ok(Json(stats))
}

#[axum::debug_handler]
pub async fn update_event(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
    Json(payload): Json<UpdateEventRequest>,
) -> Result<Json<Event>, CoreError> {
    let event = events::get_event(&state.pool, id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let updated = events::update_event(
        &state.pool,
        id,
        user.id,
        payload.name.as_deref(),
        payload.event_date,
        payload.org_name.as_deref(),
        payload.accent_color.as_deref(),
    ).await?
    .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    Ok(Json(updated))
}