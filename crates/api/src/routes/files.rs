use axum::{
    extract::{State, Path},
    response::Response,
    http::header,
    body::Body,
};
use uuid::Uuid;
use crate::state::AppState;
use crate::routes::auth::AuthUser;
use credbrew_core::{
    db::credentials,
    models::CredentialStatus,
    error::CoreError,
};

#[axum::debug_handler]
pub async fn download_credential(
    State(state): State<std::sync::Arc<AppState>>,
    Path(credential_id): Path<Uuid>,
    user: AuthUser,
) -> Result<Response, CoreError> {
    let cred = credentials::get_credential(&state.pool, credential_id).await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let details = credentials::get_credential_with_details(&state.pool, credential_id).await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let event = details.3;
    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    if cred.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    let file_key = cred.file_key.ok_or_else(|| CoreError::NotFound("credential file not found".into()))?;
    let bytes = state.storage.get(&file_key).await?;

    let content_type = if file_key.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    };

    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        content_type.parse().unwrap(),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"credential-{}.png\"", credential_id).parse().unwrap(),
    );
    Ok(response)
}

#[axum::debug_handler]
pub async fn download_template(
    State(state): State<std::sync::Arc<AppState>>,
    Path(file_key): Path<String>,
    user: AuthUser,
) -> Result<axum::response::Response, CoreError> {
    // Verify the template belongs to one of the user's events
    let template = credbrew_core::db::templates::get_template_by_file_key(&state.pool, &file_key).await?
        .ok_or_else(|| CoreError::NotFound("template not found".into()))?;

    let pt = credbrew_core::db::participant_types::get_participant_type(&state.pool, template.participant_type_id).await?
        .ok_or_else(|| CoreError::NotFound("participant type not found".into()))?;

    let event = credbrew_core::db::events::get_event(&state.pool, pt.event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let bytes = state.storage.get(&file_key).await?;

    let mut response = axum::response::Response::new(axum::body::Body::from(bytes));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        "image/svg+xml".parse().unwrap(),
    );
    response.headers_mut().insert(
        axum::http::header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", file_key.split('/').next_back().unwrap_or("template.svg")).parse().unwrap(),
    );
    Ok(response)
}