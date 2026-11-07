//! v1.0 Phase 2 — DID + signing key management routes.
//!
//! Two surface areas:
//! - **Public**: `GET /tenants/{slug}/did.json` and `GET /.well-known/did.json`
//!   serve the DID document for did:web resolution. No auth required.
//! - **Protected**: `GET/POST /api/tenants/me/did`, `GET /api/tenants/me/keys`,
//!   `POST .../keys/{id}/retire|revoke`, `POST /api/tenants/me/sign-test`.
//!   These are the tenant admin's DID management UI.
//!
//! See: CredBrew v1.0 Design RFC §4, §5

use crate::routes::auth::AuthUser;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::header,
    response::Response,
    Json,
};
use credbrew_core::{
    db::{issuer_dids, tenants as tenant_db},
    did::{
        self, build_did_document, decode_public_key_multibase, generate_keypair, sign_document,
        signing_key_from_seed, verify_document,
    },
    error::CoreError,
    models::{DecryptedSigningKey, IssuerDid, SigningKey},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// Public DID document endpoints (did:web resolution)
// ─────────────────────────────────────────────────────────────────────────────

/// Serve the DID document for a sub-path tenant DID:
/// `did:web:credbrew.example:tenants:techconf` → `GET /tenants/techconf/did.json`
#[axum::debug_handler]
pub async fn serve_tenant_did_document(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Response, CoreError> {
    let tenant = tenant_db::get_tenant_by_slug(&state.pool, &slug)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant not found".into()))?;

    let did = issuer_dids::get_active_did_for_tenant(&state.pool, tenant.id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant has no active DID".into()))?;

    Ok(did_document_response(did.did_document))
}

/// Serve the DID document for a root-domain tenant (single-tenant self-hosted
/// deploy where the tenant owns the whole domain): `GET /.well-known/did.json`.
/// Returns the document for the first tenant on the instance, or 404 if none.
#[axum::debug_handler]
pub async fn serve_root_did_document(
    State(state): State<Arc<AppState>>,
) -> Result<Response, CoreError> {
    // Pick the oldest tenant — for single-tenant self-hosted deploys this is
    // the only one. For multi-tenant SaaS, root-domain DIDs are not the right
    // pattern; tenants should use the /tenants/{slug}/did.json path instead.
    let tenants = sqlx::query_as::<_, credbrew_core::models::Tenant>(
        "SELECT id, slug, name, domain, plan, anchoring_enabled, created_at FROM tenants ORDER BY created_at LIMIT 1"
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(CoreError::Database)?;

    let Some(tenant) = tenants else {
        return Err(CoreError::NotFound(
            "no tenants exist on this instance".into(),
        ));
    };

    let did = issuer_dids::get_active_did_for_tenant(&state.pool, tenant.id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant has no active DID".into()))?;

    Ok(did_document_response(did.did_document))
}

fn did_document_response(doc: Value) -> Response {
    let body = serde_json::to_vec(&doc).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::new(axum::body::Body::from(body));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/ld+json".parse().unwrap());
    // DID documents are stable; cache aggressively.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "public, max-age=300".parse().unwrap(),
    );
    response
}

// ─────────────────────────────────────────────────────────────────────────────
// Protected: tenant admin DID management
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DidResponse {
    #[serde(flatten)]
    pub did: IssuerDid,
    pub active_key: Option<SigningKeyPublic>,
}

/// A SigningKey with the encrypted_seed field stripped (never exposed).
#[derive(Serialize)]
pub struct SigningKeyPublic {
    pub id: uuid::Uuid,
    pub issuer_did_id: uuid::Uuid,
    pub key_id: String,
    pub public_key_multibase: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub retired_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<SigningKey> for SigningKeyPublic {
    fn from(k: SigningKey) -> Self {
        SigningKeyPublic {
            id: k.id,
            issuer_did_id: k.issuer_did_id,
            key_id: k.key_id,
            public_key_multibase: k.public_key_multibase,
            status: k.status.as_str().to_string(),
            created_at: k.created_at,
            retired_at: k.retired_at,
        }
    }
}

/// Get the active issuer DID + active signing key for the current tenant.
#[axum::debug_handler]
pub async fn get_active_did(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> Result<Json<Option<DidResponse>>, CoreError> {
    let tenant_id = user.tenant_id()?;

    let did = match issuer_dids::get_active_did_for_tenant(&state.pool, tenant_id).await? {
        Some(d) => d,
        None => return Ok(Json(None)),
    };

    let active_key = issuer_dids::get_active_signing_key(&state.pool, did.id).await?;
    Ok(Json(Some(DidResponse {
        did,
        active_key: active_key.map(Into::into),
    })))
}

#[derive(Deserialize)]
pub struct MintDidRequest {
    /// Optional explicit domain. Defaults to the instance's PUBLIC_BASE_URL host.
    pub domain: Option<String>,
    /// Optional explicit path segments (e.g. ["tenants", "techconf"]).
    /// Defaults to ["tenants", {tenant_slug}].
    pub path_segments: Option<Vec<String>>,
}

/// Mint a new did:web identifier for the tenant, generate an Ed25519 keypair,
/// seal the seed with the KeyStore, persist everything, and return the DID
/// document. The previously-active DID (if any) is deactivated.
#[axum::debug_handler]
pub async fn mint_did(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<MintDidRequest>,
) -> Result<Json<DidResponse>, CoreError> {
    let tenant_id = user.tenant_id()?;
    let tenant = tenant_db::get_tenant(&state.pool, tenant_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant not found".into()))?;

    // Resolve the domain and path segments for the did:web identifier.
    let base_url_host =
        host_of(&state.config.public_base_url).unwrap_or_else(|| "localhost".to_string());
    let domain = payload.domain.unwrap_or(base_url_host);
    let path_segments: Vec<String> = payload
        .path_segments
        .unwrap_or_else(|| vec!["tenants".to_string(), tenant.slug.clone()]);
    let path_refs: Vec<&str> = path_segments.iter().map(|s| s.as_str()).collect();

    let did_string = did::build_did_web(&domain, &path_refs);

    // Generate a fresh Ed25519 keypair.
    let kp = generate_keypair();

    // Build the DID document.
    let key_id_fragment = "key-1";
    let did_document = build_did_document(&did_string, key_id_fragment, &kp.public_key_multibase);

    // Persist the DID.
    let issuer_did =
        issuer_dids::create_issuer_did(&state.pool, tenant_id, &did_string, "web", &did_document)
            .await?;

    // Seal the seed and persist the signing key.
    let sealed = state.keystore.seal(&kp.seed)?;
    let signing_key = issuer_dids::create_signing_key(
        &state.pool,
        issuer_did.id,
        key_id_fragment,
        &kp.public_key_multibase,
        &sealed,
    )
    .await?;

    tracing::info!(
        tenant_id = %tenant_id,
        did = %did_string,
        "Minted new issuer DID"
    );

    Ok(Json(DidResponse {
        did: issuer_did,
        active_key: Some(signing_key.into()),
    }))
}

/// List all signing keys for the active DID (active, retired, revoked).
#[axum::debug_handler]
pub async fn list_signing_keys(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> Result<Json<Vec<SigningKeyPublic>>, CoreError> {
    let tenant_id = user.tenant_id()?;
    let did = issuer_dids::get_active_did_for_tenant(&state.pool, tenant_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant has no active DID".into()))?;

    let keys = issuer_dids::list_signing_keys_for_did(&state.pool, did.id).await?;
    Ok(Json(keys.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
pub async fn retire_key(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<SigningKeyPublic>, CoreError> {
    let tenant_id = user.tenant_id()?;
    let did = issuer_dids::get_active_did_for_tenant(&state.pool, tenant_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant has no active DID".into()))?;

    let key = issuer_dids::retire_signing_key(&state.pool, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("signing key not found or not active".into()))?;

    // Verify the key belongs to this tenant's DID.
    if key.issuer_did_id != did.id {
        return Err(CoreError::Forbidden(
            "key does not belong to active DID".into(),
        ));
    }

    Ok(Json(key.into()))
}

#[axum::debug_handler]
pub async fn revoke_key(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<SigningKeyPublic>, CoreError> {
    let tenant_id = user.tenant_id()?;
    let did = issuer_dids::get_active_did_for_tenant(&state.pool, tenant_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant has no active DID".into()))?;

    let key = issuer_dids::revoke_signing_key(&state.pool, id)
        .await?
        .ok_or_else(|| CoreError::NotFound("signing key not found".into()))?;

    if key.issuer_did_id != did.id {
        return Err(CoreError::Forbidden(
            "key does not belong to active DID".into(),
        ));
    }

    // TODO: rebuild and re-serve the DID document without the revoked key.
    // For now, the document is stale until refreshed; verifiers that fetch
    // the document will still see the key, but a separate revocation check
    // (Phase 4 status lists) is the real defense.

    Ok(Json(key.into()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Sign-test endpoint: proves the DID + signing pipeline works end-to-end.
// Returns a sample signed VC + verifies it back. The Phase 3 issuance flow
// will replace this with real credential issuance.
// ─────────────────────────────────────────────────────────────────────────────

#[axum::debug_handler]
pub async fn sign_test(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> Result<Json<Value>, CoreError> {
    let tenant_id = user.tenant_id()?;
    let tenant = tenant_db::get_tenant(&state.pool, tenant_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant not found".into()))?;

    // Load the decrypted active signing key.
    let dk: DecryptedSigningKey = issuer_dids::get_decrypted_active_signing_key(
        &state.pool,
        tenant_id,
        state.keystore.as_ref(),
    )
    .await?;

    let signing_key = signing_key_from_seed(&dk.seed);
    let verification_method = format!("{}#{}", dk.did, dk.key_id);

    // Build a sample VC.
    let credential_id = format!(
        "{}/api/test/{}",
        state.config.public_base_url.trim_end_matches('/'),
        uuid::Uuid::new_v4()
    );
    let sample_vc = json!({
        "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://purl.imsglobal.org/spec/ob/v3p0/context.json"
        ],
        "id": credential_id,
        "type": ["VerifiableCredential", "OpenBadgeCredential"],
        "issuer": {
            "id": dk.did,
            "type": "Profile",
            "name": tenant.name
        },
        "issuanceDate": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "credentialSubject": {
            "type": "AchievementSubject",
            "id": "mailto:test@example.com",
            "achievement": {
                "id": format!("{}/achievements/test", state.config.public_base_url.trim_end_matches('/')),
                "type": "Achievement",
                "name": "CredBrew Signing Pipeline Test",
                "criteria": { "narrative": "Demonstrates that the tenant can mint a signed VC" }
            }
        }
    });

    // Sign.
    let (proof, signed_vc) = sign_document(&sample_vc, &verification_method, &signing_key)?;

    // Verify it back (sanity check).
    let public_key = decode_public_key_multibase(
        &issuer_dids::get_active_signing_key(
            &state.pool,
            issuer_dids::get_active_did_for_tenant(&state.pool, tenant_id)
                .await?
                .ok_or_else(|| CoreError::NotFound("no did".into()))?
                .id,
        )
        .await?
        .ok_or_else(|| CoreError::NotFound("no key".into()))?
        .public_key_multibase,
    )?;
    let verified = verify_document(&signed_vc, &public_key)?;

    Ok(Json(json!({
        "signed_vc": signed_vc,
        "proof": proof,
        "verified": verified,
        "verification_method": verification_method,
    })))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn host_of(url: &str) -> Option<String> {
    let stripped = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let host = stripped.split('/').next()?;
    let host = host.split(':').next()?; // strip port
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_of() {
        assert_eq!(
            host_of("http://localhost:8080"),
            Some("localhost".to_string())
        );
        assert_eq!(
            host_of("https://credbrew.example/api"),
            Some("credbrew.example".to_string())
        );
        assert_eq!(host_of("not-a-url"), None);
    }
}
