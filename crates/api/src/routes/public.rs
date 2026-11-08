use crate::state::AppState;
use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
    Json,
};
use chrono::Datelike;
use credbrew_core::{
    db::{credentials, scan_events},
    error::CoreError,
    models::CredentialStatus,
};
use serde::Serialize;

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

fn linkedin_urls(
    state: &AppState,
    code: &str,
    org_name: &str,
    event_date: Option<chrono::NaiveDate>,
    linkedin_name: &str,
) -> (String, String) {
    let verify_url = format!(
        "{}/v/{}",
        state.config.public_base_url.trim_end_matches('/'),
        code
    );

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
    let details = credentials::get_credential_by_short_code(&state.pool, &code)
        .await?
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
    // v1.0 Phase 4: cryptographic verification details (empty string = not present)
    issuer_did: String,
    proof_created: String,
    verification_method: String,
    proof_value_preview: String,
    vc_download_url: String,
    // v1.0 Phase 6: on-chain anchoring (empty string = not anchored)
    anchored_chain: String,
    anchored_explorer_url: String,
}

#[axum::debug_handler]
pub async fn verify_page(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code)
        .await?
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

    let vc_download_url = format!(
        "{}/api/public/vc/{}",
        state.config.public_base_url.trim_end_matches('/'),
        code
    );

    // v1.0 Phase 4: extract proof details from the signed VC (if present).
    let (issuer_did, proof_created, verification_method, proof_value_preview) =
        if let Some(vc_json) = &credential.vc_json {
            let issuer = vc_json["issuer"]["id"].as_str().unwrap_or("").to_string();
            let proof = &vc_json["proof"];
            (
                issuer,
                proof["created"].as_str().unwrap_or("").to_string(),
                proof["verificationMethod"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                proof["proofValue"]
                    .as_str()
                    .map(|s| s.chars().take(24).collect::<String>())
                    .unwrap_or_default(),
            )
        } else {
            (String::new(), String::new(), String::new(), String::new())
        };

    // v1.0 Phase 6: check for on-chain anchoring.
    let (anchored_chain, anchored_explorer_url) = if let Some(ref chain) = credential.anchored_chain
    {
        if let Some(ref txid) = credential.anchored_txid {
            let explorer = match chain.as_str() {
                "polygon" => format!("https://polygonscan.com/tx/{}", txid),
                "arweave" => format!("https://viewblock.io/arweave/tx/{}", txid),
                _ => "#".to_string(),
            };
            (chain.clone(), explorer)
        } else {
            (chain.clone(), String::new())
        }
    } else {
        (String::new(), String::new())
    };

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
        issuer_did,
        proof_created,
        verification_method,
        proof_value_preview,
        vc_download_url,
        anchored_chain,
        anchored_explorer_url,
    };

    let html = tmpl
        .render()
        .map_err(|e| CoreError::Internal(e.to_string()))?;

    let mut response = Html(html).into_response();
    response
        .headers_mut()
        .insert("Cache-Control", "public, max-age=300".parse().unwrap());
    Ok(response)
}

fn serve_stored_file(file_key: &str, bytes: Vec<u8>, cache_control: &'static str) -> Response {
    let (mime, as_attachment) = if file_key.ends_with(".png") {
        ("image/png", false)
    } else if file_key.ends_with(".svg") {
        ("image/svg+xml", false)
    } else {
        ("application/octet-stream", true)
    };

    let mut response = Response::new(axum::body::Body::from(bytes));
    response
        .headers_mut()
        .insert(axum::http::header::CONTENT_TYPE, mime.parse().unwrap());
    response
        .headers_mut()
        .insert("Cache-Control", cache_control.parse().unwrap());
    if as_attachment {
        let filename = file_key.split('/').next_back().unwrap_or("credential");
        response.headers_mut().insert(
            axum::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename)
                .parse()
                .unwrap(),
        );
    }
    response
}

#[axum::debug_handler]
pub async fn credential_asset(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<Response, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, _, _, _, _) = details;

    if credential.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    let file_key = credential
        .file_key
        .ok_or_else(|| CoreError::NotFound("credential file not found".into()))?;
    let bytes = state.storage.get(&file_key).await?;

    Ok(serve_stored_file(&file_key, bytes, "public, max-age=300"))
}

#[axum::debug_handler]
pub async fn og_image(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<Response, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, _, _, _, _) = details;

    if credential.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    let file_key = credential
        .file_key
        .ok_or_else(|| CoreError::NotFound("credential file not found".into()))?;
    let bytes = state.storage.get(&file_key).await?;

    Ok(serve_stored_file(&file_key, bytes, "public, max-age=86400"))
}

/// v1.0 Phase 3: Download the raw Verifiable Credential as .vc.json.
/// Returns the full signed VC document (with proof) for import into any
/// OB 3.0-compatible wallet (Badgr, Spruce, Disbadge, etc.).
#[axum::debug_handler]
pub async fn download_vc(
    State(state): State<std::sync::Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<Response, CoreError> {
    let details = credentials::get_credential_by_short_code(&state.pool, &code)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, _, _, _, _) = details;

    if credential.status == CredentialStatus::Revoked {
        return Err(CoreError::Gone("credential revoked".into()));
    }

    let vc_json = credential.vc_json.ok_or_else(|| {
        CoreError::NotFound(
            "credential has no Verifiable Credential (issued before VC support was enabled)".into(),
        )
    })?;

    let body = serde_json::to_vec_pretty(&vc_json)
        .map_err(|e| CoreError::Internal(format!("failed to serialize VC: {e}")))?;

    let mut response = Response::new(axum::body::Body::from(body));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        "application/ld+json".parse().unwrap(),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        "public, max-age=300".parse().unwrap(),
    );
    // Suggest a filename for download.
    let filename = format!("credential-{}.vc.json", code);
    response.headers_mut().insert(
        axum::http::header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{filename}\"")
            .parse()
            .unwrap(),
    );
    Ok(response)
}

/// v1.0 Phase 4: Serve the signed StatusListCredential at
/// /tenants/{slug}/status/{index}. Verifiers fetch this to check if a
/// credential has been revoked (BitstringStatusList 2021).
#[axum::debug_handler]
pub async fn serve_status_list_credential(
    State(state): State<std::sync::Arc<AppState>>,
    Path((slug, index)): Path<(String, i32)>,
) -> Result<Response, CoreError> {
    use base64::Engine;
    use credbrew_core::db::tenants as tenant_db;

    let tenant = tenant_db::get_tenant_by_slug(&state.pool, &slug)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant not found".into()))?;

    let list = sqlx::query_as::<_, credbrew_core::models::StatusList>(
        r#"
        SELECT id, tenant_id, index, bitstring, vc_id, updated_at
        FROM status_lists
        WHERE tenant_id = $1 AND index = $2
        "#,
    )
    .bind(tenant.id)
    .bind(index)
    .fetch_optional(&state.pool)
    .await
    .map_err(CoreError::Database)?
    .ok_or_else(|| CoreError::NotFound("status list not found".into()))?;

    let encoded_list = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&list.bitstring);

    let issuer_did =
        credbrew_core::db::issuer_dids::get_active_did_for_tenant(&state.pool, tenant.id)
            .await?
            .map(|d| d.did)
            .unwrap_or_else(|| format!("did:web:{}", tenant.slug));

    let status_list_vc = serde_json::json!({
        "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://w3id.org/vc/status-list/2021/v1"
        ],
        "id": list.vc_id,
        "type": ["VerifiableCredential", "BitstringStatusListCredential"],
        "issuer": issuer_did,
        "issuanceDate": list.updated_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "credentialSubject": {
            "id": format!("{}#list", list.vc_id),
            "type": "BitstringStatusList",
            "statusPurpose": "revocation",
            "encodedList": encoded_list
        }
    });

    let body = serde_json::to_vec_pretty(&status_list_vc)
        .map_err(|e| CoreError::Internal(format!("failed to serialize status list VC: {e}")))?;

    let mut response = Response::new(axum::body::Body::from(body));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        "application/ld+json".parse().unwrap(),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        "public, max-age=300".parse().unwrap(),
    );
    Ok(response)
}
