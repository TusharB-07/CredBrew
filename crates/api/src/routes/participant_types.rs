use axum::{
    extract::{State, Path},
    Json,
};
use uuid::Uuid;
use serde::Deserialize;
use crate::state::AppState;
use credbrew_core::{
    db::{participant_types, events, participants},
    models::ParticipantType,
    error::CoreError,
};
use crate::routes::auth::AuthUser;

#[derive(Deserialize)]
pub struct CreateTypeRequest {
    pub name: String,
    pub badge_enabled: bool,
    pub certificate_enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateTypeRequest {
    pub name: Option<String>,
    pub badge_enabled: Option<bool>,
    pub certificate_enabled: Option<bool>,
}

#[axum::debug_handler]
pub async fn create_type(
    State(state): State<std::sync::Arc<AppState>>,
    Path(event_id): Path<Uuid>,
    user: AuthUser,
    Json(payload): Json<CreateTypeRequest>,
) -> Result<Json<ParticipantType>, CoreError> {
    let event = events::get_event(&state.pool, event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let pt = participant_types::create_participant_type(
        &state.pool,
        event_id,
        &payload.name,
        payload.badge_enabled,
        payload.certificate_enabled,
    ).await?;

    Ok(Json(pt))
}

#[axum::debug_handler]
pub async fn update_type(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
    Json(payload): Json<UpdateTypeRequest>,
) -> Result<Json<ParticipantType>, CoreError> {
    let pt = participant_types::get_participant_type(&state.pool, id).await?
        .ok_or_else(|| CoreError::NotFound("participant type not found".into()))?;

    let event = events::get_event(&state.pool, pt.event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let updated = participant_types::update_participant_type(
        &state.pool,
        id,
        payload.name.as_deref(),
        payload.badge_enabled,
        payload.certificate_enabled,
    ).await?
    .ok_or_else(|| CoreError::NotFound("participant type not found".into()))?;

    Ok(Json(updated))
}

#[axum::debug_handler]
pub async fn delete_type(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
) -> Result<(), CoreError> {
    let pt = participant_types::get_participant_type(&state.pool, id).await?
        .ok_or_else(|| CoreError::NotFound("participant type not found".into()))?;

    let event = events::get_event(&state.pool, pt.event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let has_credentials = participant_types::has_credentials_for_type(&state.pool, id).await?;
    if has_credentials {
        return Err(CoreError::Conflict("cannot delete type with issued credentials".into()));
    }

    let participant_count =
        participants::count_participants_by_event(&state.pool, pt.event_id, Some(id)).await?;
    if participant_count > 0 {
        return Err(CoreError::Conflict(
            "cannot delete type that still has participants; remove or reassign them first".into(),
        ));
    }

    participant_types::delete_participant_type(&state.pool, id).await?;

    Ok(())
}