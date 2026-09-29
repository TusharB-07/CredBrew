use std::sync::Arc;
use std::time::Duration;
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;
use tokio::signal;
use tracing::{info, error, instrument};
use credbrew_core::{
    init_tracing,
    db,
    queue::{dequeue_issue_job, init_queue_table, IssueJob},
    models::*,
    render::{inject_name, inject_qr, render_png, render_cert_png, NameZone, QrZone},
    storage::{StorageClient, Config as StorageConfig},
    error::CoreError,
};
use sqlx::PgPool;

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
                Ok(v) => v
                    .parse()
                    .map_err(|_| CoreError::Config(config::ConfigError::Message(format!("{key} must be a number")))),
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
                    if let Err(e) = process_job(job, config.clone(), storage.clone(), email_limiter.clone(), pool.clone()).await {
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
    email_limiter: Arc<RateLimiter<governor::state::NotKeyed, governor::state::InMemoryState, governor::clock::DefaultClock>>,
    pool: PgPool,
) -> Result<(), CoreError> {
    let details = db::credentials::get_credential_with_details(&pool, job.credential_id).await?
        .ok_or_else(|| CoreError::NotFound("credential not found".into()))?;

    let (credential, participant, participant_type, event, template) = details;

    if matches!(credential.status, CredentialStatus::Emailed | CredentialStatus::Revoked) {
        info!("Credential already emailed or revoked, skipping");
        return Ok(());
    }

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

    db::credentials::update_credential_status(&pool, credential.id, CredentialStatus::Rendered, Some(&file_key), None).await?;

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
        ).await?;

        db::credentials::update_credential_status(&pool, credential.id, CredentialStatus::Emailed, None, None).await?;
    }

    Ok(())
}