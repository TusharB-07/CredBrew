use axum::{
    extract::{State, Path},
    Json,
    response::{Html, IntoResponse, Response},
};
use serde::Serialize;
use chrono::Datelike;
use crate::state::AppState;
use credbrew_core::{
    db::{credentials, scan_events},
    models::CredentialStatus,
    error::CoreError,
};
use askama::Template;

#[derive(Serialize)]
pub struct VerifyResponse {
    pub verified: bool,
    pub name: String,
    pub participant_type: String,
    pub event_name: String,
    pub org_name: String,
    pub issued_at: chrono::DateTime<chrono::Utc>,
    pub credential_image_url: String,
    pub download_url: String,
    pub linkedin_add_url: String,
    pub linkedin_share_url: String,
}

fn credential_asset_url(state: &AppState, code: &str) -> String {
    format!(
        "{}/api/public/verify/asset/{}",
        state.config.public_base_url.trim_end_matches('/'),
        code
    )
}

fn linkedin_urls(state: &AppState, code: &str, org_name: &str, event_date: Option<chrono::NaiveDate>, linkedin_name: &str) -> (String, String) {
    let verify_url = format!("{}/v/{}", state.config.public_base_url.trim_end_matches('/'), code);

    let linkedin_add_url = format!(
        "https://www.linkedin.com/profile/add?startTask=CERTIFICATION_NAME&name={}&organizationName={}&issueYear={}&issueMonth={}&certUrl={}",
        percent_encoding::percent_encode(linkedin_name.as_bytes(), percent_encoding::NON_ALPHANUMERIC),
        percent_encoding::percent_encode(org_name.as_bytes(), percent_encoding::NON_ALPHANUMERIC),
        event_date.map(|d| d.year()).unwrap_or(chrono::Utc::now().year()),
        event_date.map(|d| d.month()).unwrap_or(chrono::Utc::now().month()),
        percent_encoding::percent_encode(verify_url.as_bytes(), percent_encoding::NON_ALPHANUMERIC),
    );

    let linkedin_share_url = format!(
        "https://www.linkedin.com/sharing/share-offsite/?url={}",
        percent_encoding::percent_encode(verify_url.as_bytes(), percent_encoding::NON_ALPHANUMERIC),
    );

    (linkedin_add_url, linkedin_share_url)
}

#[axum::debug_handler]
pub async fn verify_credential(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<Json<VerifyResponse>, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code).await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, participant, participant_type, event, _template) = details;

    if credential.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    scan_events::log_scan(&state.pool, &code).await?;

    let image_url = credential_asset_url(&state, &code);
    let linkedin_name = format!("{} — {}", participant_type.name, event.name);
    let (linkedin_add_url, linkedin_share_url) = linkedin_urls(
        &state,
        &code,
        &event.org_name,
        event.event_date,
        &linkedin_name,
    );

    Ok(Json(VerifyResponse {
        verified: true,
        name: participant.name,
        participant_type: participant_type.name,
        event_name: event.name,
        org_name: event.org_name,
        issued_at: credential.issued_at,
        credential_image_url: image_url.clone(),
        download_url: image_url,
        linkedin_add_url,
        linkedin_share_url,
    }))
}

#[derive(Template)]
#[template(path = "verify.html")]
struct VerifyPageTemplate {
    name: String,
    participant_type: String,
    event_name: String,
    org_name: String,
    event_date_display: String,
    issued_at_display: String,
    credential_image_url: String,
    download_url: String,
    linkedin_add_url: String,
    linkedin_share_url: String,
    public_base_url: String,
    code: String,
    og_image_url: String,
}

#[axum::debug_handler]
pub async fn verify_page(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code).await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, participant, participant_type, event, _template) = details;

    if credential.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    let image_url = credential_asset_url(&state, &code);
    let linkedin_name = format!("{} — {}", participant_type.name, event.name);
    let (linkedin_add_url, linkedin_share_url) = linkedin_urls(
        &state,
        &code,
        &event.org_name,
        event.event_date,
        &linkedin_name,
    );

    let og_image_url = format!(
        "{}/api/public/og-image/{}?v={}",
        state.config.public_base_url,
        code,
        credential.updated_at.timestamp()
    );

    let tmpl = VerifyPageTemplate {
        name: participant.name,
        participant_type: participant_type.name,
        event_name: event.name,
        org_name: event.org_name,
        event_date_display: event
            .event_date
            .map(|d| d.format("%B %d, %Y").to_string())
            .unwrap_or_else(|| "Not specified".to_string()),
        issued_at_display: credential.issued_at.format("%B %d, %Y").to_string(),
        credential_image_url: image_url.clone(),
        download_url: image_url,
        linkedin_add_url,
        linkedin_share_url,
        public_base_url: state.config.public_base_url.clone(),
        code: code.clone(),
        og_image_url,
    };

    let html = tmpl.render().map_err(|e| CoreError::Internal(e.to_string()))?;

    let mut response = Html(html).into_response();
    response.headers_mut().insert(
        "Cache-Control",
        "public, max-age=300".parse().unwrap(),
    );
    Ok(response)
}

fn serve_stored_file(
    file_key: &str,
    bytes: Vec<u8>,
    cache_control: &'static str,
) -> Response {
    let (mime, as_attachment) = if file_key.ends_with(".png") {
        ("image/png", false)
    } else if file_key.ends_with(".svg") {
        ("image/svg+xml", false)
    } else {
        ("application/octet-stream", true)
    };

    let mut response = Response::new(axum::body::Body::from(bytes));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        mime.parse().unwrap(),
    );
    response.headers_mut().insert("Cache-Control", cache_control.parse().unwrap());
    if as_attachment {
        let filename = file_key.split('/').next_back().unwrap_or("credential");
        response.headers_mut().insert(
            axum::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename).parse().unwrap(),
        );
    }
    response
}

#[axum::debug_handler]
pub async fn credential_asset(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<Response, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code).await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, _, _, _, _) = details;

    if credential.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    let file_key = credential.file_key.ok_or_else(|| CoreError::NotFound("credential file not found".into()))?;
    let bytes = state.storage.get(&file_key).await?;

    Ok(serve_stored_file(&file_key, bytes, "public, max-age=300"))
}

#[axum::debug_handler]
pub async fn og_image(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<Response, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code).await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, _, _, _, _) = details;

    if credential.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    let file_key = credential.file_key.ok_or_else(|| CoreError::NotFound("credential file not found".into()))?;
    let bytes = state.storage.get(&file_key).await?;

    Ok(serve_stored_file(&file_key, bytes, "public, max-age=86400"))
}