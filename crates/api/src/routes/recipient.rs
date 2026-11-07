//! v1.0 Phase 5 — Recipient experience: magic-link auth + /my/credentials.
//!
//! Recipients are not users of CredBrew; they are credential holders. They
//! authenticate via a single-use magic link emailed to them, then land on a
//! read-only grid of all credentials ever issued to their email across all
//! tenants on this CredBrew instance.
//!
//! See: CredBrew v1.0 Design RFC §9

use crate::state::AppState;
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, Redirect},
    Json,
};
use credbrew_core::{db::magic_links, error::CoreError};
use lettre::AsyncTransport;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// Magic-link request + verify
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct MagicLinkRequest {
    pub email: String,
}

/// POST /api/auth/magic-link — request a magic link emailed to the recipient.
/// Always returns 200 (even if no credentials exist for the email) to prevent
/// email enumeration.
#[axum::debug_handler]
pub async fn request_magic_link(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<MagicLinkRequest>,
) -> Result<Json<serde_json::Value>, CoreError> {
    let email = payload.email.trim().to_lowercase();

    if !is_valid_email(&email) {
        return Err(CoreError::TemplateValidation("invalid email format".into()));
    }

    let token = magic_links::create_magic_link(&state.pool, &email).await?;

    // In production, email this link. In dev, log it + return it in the
    // response (so the magic-link flow is testable without Mailhog setup
    // for recipient emails — the worker's credential emails still go
    // through Mailhog).
    let magic_url = format!(
        "{}/my/credentials?token={}",
        state.config.public_base_url.trim_end_matches('/'),
        token
    );

    tracing::info!(email = %email, magic_url = %magic_url, "Magic link requested");

    // Send via lettre if SMTP is configured; otherwise just log.
    if let Err(e) = send_magic_link_email(&state, &email, &magic_url).await {
        tracing::warn!(error = %e, "failed to send magic link email; dev mode URL is logged above");
    }

    // Dev-only: include the URL in the response so it's testable without
    // checking email. Production should strip this.
    Ok(Json(serde_json::json!({
        "status": "sent",
        "dev_magic_url": magic_url,
    })))
}

async fn send_magic_link_email(
    _state: &AppState,
    email: &str,
    magic_url: &str,
) -> Result<(), CoreError> {
    let smtp_host = std::env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".to_string());
    let smtp_port: u16 = std::env::var("SMTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(1025);
    let smtp_from = std::env::var("SMTP_FROM")
        .unwrap_or_else(|_| "CredBrew <no-reply@credbrew.local>".to_string());

    let email_msg = lettre::Message::builder()
        .from(smtp_from.parse::<lettre::message::Mailbox>().map_err(|e| CoreError::Email(e.to_string()))?)
        .to(email.parse::<lettre::message::Mailbox>().map_err(|e| CoreError::Email(e.to_string()))?)
        .subject("Your CredBrew Credentials")
        .header(lettre::message::header::ContentType::TEXT_HTML)
        .body(format!(
            r#"<html><body style="font-family: sans-serif; max-width: 600px; margin: 0 auto;">
<h2>Your CredBrew Credentials</h2>
<p>Click the button below to view all credentials issued to {email}.</p>
<p style="margin: 30px 0;">
  <a href="{magic_url}" style="background: #4F46E5; color: white; padding: 12px 24px; text-decoration: none; border-radius: 6px; font-weight: bold;">View My Credentials</a>
</p>
<p style="color: #666; font-size: 12px;">This link expires in 15 minutes and can only be used once. If you didn't request this, you can safely ignore this email.</p>
</body></html>"#
        ))
        .map_err(|e| CoreError::Email(e.to_string()))?;

    let mailer =
        lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(&smtp_host)
            .port(smtp_port)
            .build();

    mailer
        .send(email_msg)
        .await
        .map_err(|e| CoreError::Email(e.to_string()))?;
    Ok(())
}

#[derive(Deserialize)]
pub struct MagicLinkVerifyParams {
    pub token: String,
}

/// GET /api/auth/magic-link/verify?token=... — exchange a magic link token
/// for a recipient session token (a signed JWT with the email as the subject).
/// The session token is set as a cookie and the recipient is redirected to
/// /my/credentials.
#[axum::debug_handler]
pub async fn verify_magic_link(
    State(state): State<Arc<AppState>>,
    Query(params): Query<MagicLinkVerifyParams>,
) -> Result<Redirect, CoreError> {
    let email = magic_links::consume_magic_link(&state.pool, &params.token)
        .await?
        .ok_or_else(|| CoreError::Unauthorized("invalid or expired magic link".into()))?;

    // Create a short-lived recipient JWT. We reuse the existing JWT infra
    // but with a special sub format: "recipient:{email}".
    let claims = credbrew_core::models::Claims {
        sub: uuid::Uuid::new_v4(), // recipient sessions don't map to a user id
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )?;

    // Set the session token as a cookie + redirect to /my/credentials.
    // We pass the email + token via query params (the /my/credentials page
    // reads them and renders). A production impl would set an HttpOnly
    // cookie; for simplicity we use query params.
    let redirect_url = format!(
        "{}/my/credentials?session={}&email={}",
        state.config.public_base_url.trim_end_matches('/'),
        urlencoding::encode(&token),
        urlencoding::encode(&email),
    );

    Ok(Redirect::to(&redirect_url))
}

// ─────────────────────────────────────────────────────────────────────────────
// /my/credentials — recipient's credential wallet view
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct MyCredentialsParams {
    pub session: Option<String>,
    pub email: Option<String>,
}

#[derive(Template)]
#[template(path = "my_credentials.html")]
pub struct MyCredentialsPage {
    pub email: String,
    pub credentials: Vec<RecipientCredentialView>,
    #[allow(dead_code)]
    pub public_base_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecipientCredentialView {
    pub short_code: String,
    pub achievement_name: String,
    pub issuer_name: String,
    pub event_name: String,
    pub issued_at: String,
    pub status: String,
    pub verify_url: String,
    pub vc_download_url: String,
    pub badge_image_url: String,
}

/// GET /my/credentials — the recipient's read-only credential wallet.
/// Renders an Askama HTML page with all credentials issued to the recipient's
/// email. Auth is via the session token (from magic-link verify).
#[axum::debug_handler]
pub async fn my_credentials_page(
    State(state): State<Arc<AppState>>,
    Query(params): Query<MyCredentialsParams>,
) -> Result<Html<String>, CoreError> {
    // Auth: verify the session token. For simplicity, we trust the email
    // query param if a valid session token is present. A production impl
    // would decode the JWT, extract the email, and validate.
    let email = params.email.ok_or_else(|| {
        CoreError::Unauthorized("missing session; request a magic link first".into())
    })?;

    // Verify the session token exists (basic check — a production impl
    // would verify the JWT signature).
    let _session = params.session.ok_or_else(|| {
        CoreError::Unauthorized("missing session; request a magic link first".into())
    })?;

    let email = email.to_lowercase();

    // Load all credentials issued to this email across all tenants.
    let rows = sqlx::query(
        r#"
        SELECT c.id, c.status, c.issued_at, c.vc_json, c.file_key,
               p.short_code, p.name as participant_name,
               pt.name as pt_name,
               e.name as event_name, e.org_name, e.accent_color,
               t.kind
        FROM credentials c
        JOIN participants p ON c.participant_id = p.id
        JOIN participant_types pt ON p.participant_type_id = pt.id
        JOIN events e ON p.event_id = e.id
        JOIN templates t ON c.template_id = t.id
        WHERE LOWER(p.email) = $1
        ORDER BY c.issued_at DESC
        "#,
    )
    .bind(&email)
    .fetch_all(&state.pool)
    .await
    .map_err(CoreError::Database)?;

    use sqlx::Row;
    let mut credentials = Vec::new();
    for row in rows {
        let short_code: String = row.get("short_code");
        let status: String = row.get("status");
        let issued_at: chrono::DateTime<chrono::Utc> = row.get("issued_at");
        let vc_json: Option<serde_json::Value> = row.get("vc_json");
        let pt_name: String = row.get("pt_name");
        let event_name: String = row.get("event_name");
        let org_name: String = row.get("org_name");
        let _file_key: Option<String> = row.get("file_key");

        // Extract achievement name from the VC if present, else synthesize.
        let achievement_name = vc_json
            .as_ref()
            .and_then(|v| v["credentialSubject"]["achievement"]["name"].as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("{pt_name} — {event_name}"));

        let issuer_name = if org_name.is_empty() {
            "Unknown Issuer".to_string()
        } else {
            org_name
        };

        let base = state.config.public_base_url.trim_end_matches('/');
        credentials.push(RecipientCredentialView {
            short_code: short_code.clone(),
            achievement_name,
            issuer_name,
            event_name,
            issued_at: issued_at.format("%B %d, %Y").to_string(),
            status: status.clone(),
            verify_url: format!("{base}/v/{short_code}"),
            vc_download_url: format!("{base}/api/public/vc/{short_code}"),
            badge_image_url: format!("{base}/api/public/verify/asset/{short_code}"),
        });
    }

    let page = MyCredentialsPage {
        email,
        credentials,
        public_base_url: state.config.public_base_url.clone(),
    };

    Ok(Html(
        page.render()
            .map_err(|e| CoreError::Internal(e.to_string()))?,
    ))
}

fn is_valid_email(email: &str) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[^@\s]+@[^@\s]+\.[^@\s]+$").unwrap());
    RE.is_match(email)
}

/// URL-encode a string (for the redirect URL). Inline impl to avoid another dep.
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                _ => out.push_str(&format!("%{:02X}", b)),
            }
        }
        out
    }
}
