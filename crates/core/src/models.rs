use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub event_date: Option<NaiveDate>,
    pub org_name: String,
    pub accent_color: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ParticipantType {
    pub id: Uuid,
    pub event_id: Uuid,
    pub name: String,
    pub badge_enabled: bool,
    pub certificate_enabled: bool,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Participant {
    pub id: Uuid,
    pub event_id: Uuid,
    pub participant_type_id: Uuid,
    pub name: String,
    pub email: String,
    pub short_code: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Template {
    pub id: Uuid,
    pub participant_type_id: Uuid,
    pub kind: TemplateKind,
    pub file_key: String,
    pub name_zone: serde_json::Value,
    pub qr_zone: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum TemplateKind {
    Badge,
    Certificate,
}

impl TemplateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TemplateKind::Badge => "badge",
            TemplateKind::Certificate => "certificate",
        }
    }
}

impl std::fmt::Display for TemplateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TemplateKind {
    type Err = crate::error::CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "badge" => Ok(TemplateKind::Badge),
            "certificate" => Ok(TemplateKind::Certificate),
            other => Err(crate::error::CoreError::TemplateValidation(format!(
                "unknown template kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Credential {
    pub id: Uuid,
    pub participant_id: Uuid,
    pub template_id: Uuid,
    pub kind: TemplateKind,
    pub file_key: Option<String>,
    pub status: CredentialStatus,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub issued_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum CredentialStatus {
    Pending,
    Rendered,
    Emailed,
    Failed,
    Revoked,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ScanEvent {
    pub id: i64,
    pub short_code: String,
    pub scanned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameZone {
    pub x_pct: f32,
    pub y_pct: f32,
    pub w_pct: f32,
    pub font_size_pct: f32,
    pub color: String,
    pub align: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrZone {
    pub x_pct: f32,
    pub y_pct: f32,
    pub size_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub exp: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: String,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            email: user.email,
            name: user.name,
        }
    }
}