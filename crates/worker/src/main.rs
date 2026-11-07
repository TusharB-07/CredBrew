use credbrew_core::{
    crypto::{KeyStore, LocalFileKeyStore},
    db,
    did::{decode_public_key_multibase, sign_document, signing_key_from_seed},
    error::CoreError,
    init_tracing,
    models::*,
    queue::{dequeue_issue_job, init_queue_table, IssueJob},
    render::{inject_name, inject_qr, render_cert_png, render_png, NameZone, QrZone},
    storage::{Config as StorageConfig, StorageClient},
    vc::{build_ob30_credential, status_list_credential_url, VcInputs},
};
use governor::{Quota, RateLimiter};
use sqlx::PgPool;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{error, info, instrument};

mod email;

#[derive(Clone)]
struct WorkerConfig {
    storage: StorageConfig,
    smtp_host: String,
    smtp_port: u16,
    smtp_user: String,
    smtp_pass: String,
    smtp_from: String,
    public_base_url: String,
    email_per_min: u32,
}

impl WorkerConfig {
    fn from_env() -> Result<Self, CoreError> {
        let parse_or = |key: &str, default: u16| -> Result<u16, CoreError> {
            match std::env::var(key) {
                Ok(v) => v.parse().map_err(|_| {
                    CoreError::Config(config::ConfigError::Message(format!(
                        "{key} must be a number"
                    )))
                }),
                Err(_) => Ok(default),
            }
        };

        Ok(Self {
            storage: StorageConfig::from_env()?,
            smtp_host: std::env::var("SMTP_HOST")?,
            smtp_port: parse_or("SMTP_PORT", 1025)?,
            smtp_user: std::env::var("SMTP_USER").unwrap_or_default(),
            smtp_pass: std::env::var("SMTP_PASS").unwrap_or_default(),
            smtp_from: std::env::var("SMTP_FROM")?,
            public_base_url: std::env::var("PUBLIC_BASE_URL")?,
            email_per_min: std::env::var("EMAIL_PER_MIN")
                .ok()
                .map(|v| {
                    v.parse::<u32>().map_err(|_| {
                        CoreError::Config(config::ConfigError::Message(
                            "EMAIL_PER_MIN must be a number".into(),
                        ))
                    })
                })
                .transpose()?
                .unwrap_or(60),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), CoreError> {
    init_tracing();

    let config = WorkerConfig::from_env()?;
    let pool = PgPool::connect(&std::env::var("DATABASE_URL")?).await?;

    init_queue_table(&pool).await?;

    let storage = StorageClient::new(&config.storage)?;

    // v1.0 Phase 2/3: the worker signs VCs, so it needs the keystore.
    let keystore: Arc<dyn KeyStore> = Arc::new(LocalFileKeyStore::from_env()?);

    let email_limiter = Arc::new(RateLimiter::direct(Quota::per_minute(
        NonZeroU32::new(config.email_per_min).unwrap(),
    )));

    info!("Worker started, listening for jobs...");

    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Ok(Some(job)) = dequeue_issue_job(&pool).await {
                    info!("Processing job for credential {}", job.credential_id);
                    let credential_id = job.credential_id;
                    if let Err(e) = process_job(
                        job,
                        config.clone(),
                        storage.clone(),
                        keystore.clone(),
                        email_limiter.clone(),
                        pool.clone(),
                    ).await {
                        error!("Job failed: {}", e);
                        if let Err(db_err) = db::credentials::update_credential_status(
                            &pool,
                            credential_id,
                            CredentialStatus::Failed,
                            None,
                            Some(&e.to_string()),
                        ).await {
                            error!("Failed to record failure for credential {}: {}", credential_id, db_err);
                        }
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received");
                break;
            }
        }
    }

    info!("Worker stopped");
    Ok(())
}

#[instrument(skip_all, fields(credential_id = %job.credential_id))]
async fn process_job(
    job: IssueJob,
    config: WorkerConfig,
    storage: StorageClient,
    keystore: Arc<dyn KeyStore>,
    email_limiter: Arc<
        RateLimiter<
            governor::state::NotKeyed,
            governor::state::InMemoryState,
            governor::clock::DefaultClock,
        >,
    >,
    pool: PgPool,
) -> Result<(), CoreError> {
    let details = db::credentials::get_credential_with_details(&pool, job.credential_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, participant, participant_type, event, template) = details;

    if matches!(
        credential.status,
        CredentialStatus::Emailed | CredentialStatus::Revoked
    ) {
        info!("Credential already emailed or revoked, skipping");
        return Ok(());
    }

    // ─────────────────────────────────────────────────────────────────────
    // v1.0 Phase 3: Build + sign the Verifiable Credential (source of truth).
    // The PNG becomes a derived artifact. If the tenant has no issuer DID,
    // we skip VC signing and fall back to v0.1 PNG-only behavior.
    // ─────────────────────────────────────────────────────────────────────
    let tenant_id = event.tenant_id.ok_or_else(|| {
        CoreError::Internal("event has no tenant_id; run the v0.1 -> v1.0 backfill binary".into())
    })?;

    let active_did = db::issuer_dids::get_active_did_for_tenant(&pool, tenant_id).await?;

    if let Some(issuer_did) = active_did {
        // Load the decrypted signing key for this tenant's active DID.
        let dk =
            db::issuer_dids::get_decrypted_active_signing_key(&pool, tenant_id, keystore.as_ref())
                .await?;

        let signing_key = signing_key_from_seed(&dk.seed);
        let verification_method = format!("{}#{}", dk.did, dk.key_id);

        // Get or create the tenant's status list and allocate an index.
        // We use a simple monotonically-increasing index = (current credential
        // count for tenant) % 131072. Phase 4 will replace this with a proper
        // allocation counter on the status_lists table.
        let tenant = db::tenants::get_tenant(&pool, tenant_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("tenant not found".into()))?;
        let status_url = status_list_credential_url(&config.public_base_url, &tenant.slug, 1);

        // Get-or-create list 1 for this tenant.
        let (status_list, _created) =
            db::status_lists::get_or_create_latest_list(&pool, tenant_id, |idx| {
                status_list_credential_url(&config.public_base_url, &tenant.slug, idx)
            })
            .await?;

        // Allocate an index: count existing credentials on this list + 1.
        // Phase 4 will add a proper allocation counter.
        let status_index = next_status_list_index(&pool, status_list.id).await?;

        let issuer_name = if event.org_name.is_empty() {
            &tenant.name
        } else {
            &event.org_name
        };

        let vc_inputs = VcInputs {
            credential_id: credential.id,
            issuer_did: &dk.did,
            issuer_name,
            public_base_url: &config.public_base_url,
            participant: &participant,
            participant_type: &participant_type,
            event: &event,
            template_kind: template.kind,
            status_list_credential_url: &status_url,
            status_list_index: status_index,
        };

        let unsigned_vc = build_ob30_credential(&vc_inputs)?;
        let (proof, signed_vc) = sign_document(&unsigned_vc, &verification_method, &signing_key)?;

        // Sanity check: verify the signature back before persisting.
        let public_key = decode_public_key_multibase(
            &db::issuer_dids::get_active_signing_key(&pool, issuer_did.id)
                .await?
                .ok_or_else(|| CoreError::NotFound("no active signing key".into()))?
                .public_key_multibase,
        )?;
        if !credbrew_core::did::verify_document(&signed_vc, &public_key)? {
            return Err(CoreError::Signing(
                "self-verification failed after signing".into(),
            ));
        }

        // Persist the signed VC + status list allocation.
        let vc_id = signed_vc["id"]
            .as_str()
            .ok_or_else(|| CoreError::Signing("vc has no id".into()))?
            .to_string();
        db::credentials::set_credential_vc(
            &pool,
            credential.id,
            &vc_id,
            &signed_vc,
            status_list.id,
            status_index as i32,
        )
        .await?;

        info!(
            credential_id = %credential.id,
            vc_id = %vc_id,
            issuer_did = %dk.did,
            "Signed and persisted Verifiable Credential"
        );
        let _ = proof; // proof is embedded in signed_vc; kept for clarity
    } else {
        tracing::warn!(
            tenant_id = %tenant_id,
            "Tenant has no active issuer DID; issuing PNG-only credential without VC. \
             Mint a DID via POST /api/tenants/me/did to enable VC signing."
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // Render the PNG (derived artifact, unchanged from v0.1).
    // ─────────────────────────────────────────────────────────────────────
    let svg_bytes = storage.get(&template.file_key).await?;
    let svg_str = std::str::from_utf8(&svg_bytes)?;

    let name_zone: NameZone = serde_json::from_value(template.name_zone.clone())?;
    let qr_zone: QrZone = serde_json::from_value(template.qr_zone.clone())?;

    let mut svg_with_name = inject_name(svg_str, &participant.name, &name_zone)?;
    svg_with_name = inject_qr(
        &svg_with_name,
        &participant.short_code,
        &config.public_base_url,
        &qr_zone,
    )?;

    let (file_key, file_bytes, content_type) = match template.kind {
        TemplateKind::Badge => {
            let png = render_png(&svg_with_name)?;
            let key = format!("credentials/{}.png", credential.id);
            (key, png, "image/png")
        }
        TemplateKind::Certificate => {
            let png = render_cert_png(&svg_with_name)?;
            let key = format!("credentials/{}.png", credential.id);
            (key, png, "image/png")
        }
    };

    storage.put(&file_key, file_bytes, content_type).await?;

    db::credentials::update_credential_status(
        &pool,
        credential.id,
        CredentialStatus::Rendered,
        Some(&file_key),
        None,
    )
    .await?;

    if job.send_email {
        email_limiter.until_ready().await;
        let email_config = email::EmailConfig {
            smtp_host: config.smtp_host.clone(),
            smtp_port: config.smtp_port,
            smtp_user: config.smtp_user.clone(),
            smtp_pass: config.smtp_pass.clone(),
            smtp_from: config.smtp_from.clone(),
            public_base_url: config.public_base_url.clone(),
        };
        email::send_credential_email(
            &email_config,
            &participant,
            &participant_type,
            &event,
            &template.kind,
            &file_key,
            &storage,
        )
        .await?;

        db::credentials::update_credential_status(
            &pool,
            credential.id,
            CredentialStatus::Emailed,
            None,
            None,
        )
        .await?;
    }

    Ok(())
}

/// Allocate the next status list index for a credential. Simple heuristic:
/// count credentials already on this list + 1. Phase 4 will replace this
/// with a proper allocation counter on the status_lists table to avoid
/// races under concurrent issuance.
async fn next_status_list_index(
    pool: &PgPool,
    status_list_id: uuid::Uuid,
) -> Result<u32, CoreError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM credentials WHERE status_list_id = $1")
            .bind(status_list_id)
            .fetch_one(pool)
            .await
            .map_err(CoreError::Database)?;
    Ok(count as u32 + 1)
}
