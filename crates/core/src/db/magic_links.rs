//! Magic-link auth for recipient-only flows (the /my/credentials page).
//!
//! Recipients are not users of CredBrew — they are credential holders. Forcing
//! them to register a password to view their own credentials is hostile. Magic
//! links are single-use, 15-minute TTL, scoped to the recipient's email.
//!
//! See: CredBrew v1.0 Design RFC §9.3

use crate::error::CoreError;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

const MAGIC_LINK_TTL_MINUTES: i64 = 15;
const TOKEN_LEN: usize = 32;

/// A pending magic link (not yet consumed).
#[derive(Debug, Clone)]
pub struct MagicLink {
    pub id: Uuid,
    pub email: String,
    pub token: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub used_at: Option<chrono::DateTime<Utc>>,
}

/// Create a new magic link for an email. Returns the token (to embed in the
/// email link). The token is 32 URL-safe base64 characters.
pub async fn create_magic_link(pool: &PgPool, email: &str) -> Result<String, CoreError> {
    let token = generate_token();
    let expires_at = Utc::now() + Duration::minutes(MAGIC_LINK_TTL_MINUTES);

    sqlx::query(
        r#"
        INSERT INTO magic_links (email, token, expires_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(email.to_lowercase())
    .bind(&token)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(token)
}

/// Consume a magic link token. Returns the email if valid (not expired, not
/// already used). Marks the link as used. Returns None if the token is
/// invalid, expired, or already consumed.
pub async fn consume_magic_link(pool: &PgPool, token: &str) -> Result<Option<String>, CoreError> {
    let row = sqlx::query(
        r#"
        UPDATE magic_links
        SET used_at = now()
        WHERE token = $1
          AND used_at IS NULL
          AND expires_at > now()
        RETURNING email
        "#,
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let email: String = sqlx::Row::get(&row, "email");
        Ok(Some(email))
    } else {
        Ok(None)
    }
}

/// Purge expired or used magic links. Called periodically by the worker or
/// a cron; not strictly necessary but keeps the table small.
pub async fn purge_expired(pool: &PgPool) -> Result<u64, CoreError> {
    let result =
        sqlx::query("DELETE FROM magic_links WHERE expires_at < now() OR used_at IS NOT NULL")
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; TOKEN_LEN] = rng.gen();
    // URL-safe base64, no padding
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_is_url_safe() {
        let token = generate_token();
        assert_eq!(token.len(), 43); // 32 bytes → 43 base64 chars without padding
        assert!(token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
