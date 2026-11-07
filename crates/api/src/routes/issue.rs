use crate::routes::auth::AuthUser;
use crate::state::AppState;
use axum::{
    extract::{Json as JsonExtract, Path, Query, State},
    Json,
};
use credbrew_core::{
    db::{credentials, events, participant_types, participants, templates},
    error::CoreError,
    models::{CredentialStatus, TemplateKind},
    queue::{enqueue_issue_job, IssueJob},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct IssueRequest {
    pub participant_type_id: Uuid,
    pub kind: IssueKind,
    pub send_email: bool,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IssueKind {
    Badge,
    Certificate,
    Both,
}

#[derive(Serialize)]
pub struct IssueResponse {
    pub queued: usize,
    pub skipped: usize,
}

#[axum::debug_handler]
pub async fn issue_credentials(
    State(state): State<std::sync::Arc<AppState>>,
    Path(event_id): Path<Uuid>,
    user: AuthUser,
    JsonExtract(payload): JsonExtract<IssueRequest>,
) -> Result<Json<IssueResponse>, CoreError> {
    let event = events::get_event(&state.pool, event_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let pt = participant_types::get_participant_type(&state.pool, payload.participant_type_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("participant type not found".into()))?;

    if pt.event_id != event_id {
        return Err(CoreError::Forbidden(
            "participant type does not belong to event".into(),
        ));
    }

    let kinds = match payload.kind {
        IssueKind::Badge => vec![TemplateKind::Badge],
        IssueKind::Certificate => vec![TemplateKind::Certificate],
        IssueKind::Both => vec![TemplateKind::Badge, TemplateKind::Certificate],
    };

    let mut total_queued = 0;
    let mut total_skipped = 0;

    let participant_count = participants::count_participants_by_event(
        &state.pool,
        event_id,
        Some(payload.participant_type_id),
    )
    .await?;

    for kind in kinds {
        let template = templates::get_template_by_type_and_kind(
            &state.pool,
            payload.participant_type_id,
            kind,
        )
        .await?
        .ok_or_else(|| CoreError::TemplateValidation(format!("no template for {:?}", kind)))?;

        let ids = credentials::create_credentials_for_type(
            &state.pool,
            payload.participant_type_id,
            template.id,
            kind,
        )
        .await?;

        for id in &ids {
            let job = IssueJob {
                credential_id: *id,
                send_email: payload.send_email,
            };
            enqueue_issue_job(&state.pool, &job).await?;
        }

        total_queued += ids.len();
        total_skipped += (participant_count as usize).saturating_sub(ids.len());
    }

    Ok(Json(IssueResponse {
        queued: total_queued,
        skipped: total_skipped,
    }))
}

#[derive(Serialize)]
pub struct IssueStatusResponse {
    pub pending: i64,
    pub rendered: i64,
    pub emailed: i64,
    pub failed: i64,
    pub total: i64,
    pub recent_failures: Vec<FailureDetail>,
}

#[derive(Serialize)]
pub struct FailureDetail {
    pub credential_id: Uuid,
    pub participant_name: String,
    pub participant_email: String,
    pub error: String,
    pub attempts: i32,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[axum::debug_handler]
pub async fn issue_status(
    State(state): State<std::sync::Arc<AppState>>,
    Path(event_id): Path<Uuid>,
    Query(params): Query<StatusParams>,
    user: AuthUser,
) -> Result<Json<IssueStatusResponse>, CoreError> {
    let event = events::get_event(&state.pool, event_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let (pending, rendered, emailed, failed, total) =
        credentials::get_issue_status(&state.pool, event_id, params.type_id, params.kind).await?;

    let failures = credentials::get_recent_failures(&state.pool, event_id, 20).await?;
    let mut recent_failures = Vec::new();
    for f in failures {
        let cred = credentials::get_credential_with_details(&state.pool, f.id)
            .await?
            .unwrap();
        recent_failures.push(FailureDetail {
            credential_id: f.id,
            participant_name: cred.1.name,
            participant_email: cred.1.email,
            error: f.last_error.unwrap_or_default(),
            attempts: f.attempts,
            updated_at: f.updated_at,
        });
    }

    Ok(Json(IssueStatusResponse {
        pending,
        rendered,
        emailed,
        failed,
        total,
        recent_failures,
    }))
}

#[derive(Deserialize)]
pub struct StatusParams {
    pub type_id: Option<Uuid>,
    pub kind: Option<TemplateKind>,
}

#[axum::debug_handler]
pub async fn retry_credential(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, CoreError> {
    let cred = credentials::get_credential(&state.pool, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let details = credentials::get_credential_with_details(&state.pool, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let event = details.3;
    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    if cred.status != CredentialStatus::Failed {
        return Err(CoreError::Conflict(
            "only failed credentials can be retried".into(),
        ));
    }

    let reset = credentials::reset_credential_to_pending(&state.pool, id).await?;
    if reset.is_none() {
        return Err(CoreError::NotFound("credential not found".into()));
    }

    let job = IssueJob {
        credential_id: id,
        send_email: true,
    };
    enqueue_issue_job(&state.pool, &job).await?;

    Ok(Json(serde_json::json!({ "status": "requeued" })))
}

#[axum::debug_handler]
pub async fn revoke_credential(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
    JsonExtract(payload): JsonExtract<RevokeRequest>,
) -> Result<Json<serde_json::Value>, CoreError> {
    let cred = credentials::get_credential(&state.pool, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let details = credentials::get_credential_with_details(&state.pool, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let event = details.3;
    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    if payload.status != "revoked" {
        return Err(CoreError::TemplateValidation(
            "status must be 'revoked'".into(),
        ));
    }

    // v1.0 Phase 4: flip the BitstringStatusList bit so verifiers see the
    // revocation without re-fetching the credential row. The DB status is
    // also set to 'revoked' (defense-in-depth + fast local queries).
    if let (Some(list_id), Some(idx)) = (cred.status_list_id, cred.status_list_index) {
        credbrew_core::db::status_lists::set_bit(&state.pool, list_id, idx as u32).await?;
        tracing::info!(
            credential_id = %id,
            status_list_id = %list_id,
            status_list_index = idx,
            "Flipped revocation bit in status list"
        );
    }

    credentials::revoke_credential(&state.pool, id).await?;

    Ok(Json(serde_json::json!({ "status": "revoked" })))
}

#[derive(Deserialize)]
pub struct RevokeRequest {
    pub status: String,
}
