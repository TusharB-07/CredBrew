use crate::state::{AppConfig, AppState};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::{
    extract::{FromRequestParts, Request, State},
    http::request::Parts,
    http::HeaderName,
    middleware::Next,
    response::Response,
    Json,
};
use chrono::{Duration, Utc};
use credbrew_core::{
    db::{tenants as tenant_db, users},
    error::CoreError,
    models::{ActiveTenant, AuthResponse, Claims, User},
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::Deserialize;
use std::ops::Deref;
use std::sync::Arc;
use uuid::Uuid;

/// Header name clients use to select the active tenant for a request.
/// If absent, the middleware falls back to the user's oldest membership.
pub const TENANT_HEADER: HeaderName = HeaderName::from_static("x-tenant-id");

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
    pub active_tenant: Option<ActiveTenant>,
}

impl Deref for AuthUser {
    type Target = User;

    fn deref(&self) -> &Self::Target {
        &self.user
    }
}

impl AuthUser {
    /// The active tenant for this request. Returns 403 if the user has no
    /// tenant yet — callers should use this on routes that require a tenant
    /// context (which is all protected routes in v1.0+).
    pub fn require_tenant(&self) -> Result<&ActiveTenant, CoreError> {
        self.active_tenant
            .as_ref()
            .ok_or_else(|| CoreError::Forbidden("user has no tenant membership".into()))
    }

    /// Convenience: the active tenant's id, or an error if none.
    pub fn tenant_id(&self) -> Result<Uuid, CoreError> {
        Ok(self.require_tenant()?.id())
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = CoreError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user = parts
            .extensions
            .get::<User>()
            .cloned()
            .ok_or_else(|| CoreError::Unauthorized("missing or invalid auth token".into()))?;
        let active_tenant = parts.extensions.get::<ActiveTenant>().cloned();
        Ok(AuthUser {
            user,
            active_tenant,
        })
    }
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(RegisterRequest {
        name,
        email,
        password,
    }): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, CoreError> {
    if password.len() < 8 {
        return Err(CoreError::TemplateValidation(
            "password must be at least 8 characters".into(),
        ));
    }

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string();

    let user = users::create_user(&state.pool, &email, &password_hash, &name).await?;

    // v1.0: every new user gets a personal tenant on signup, owned by them.
    // This preserves the v0.1 single-user workflow while enabling multi-tenancy.
    let tenant_name = if name.trim().is_empty() {
        "My Workspace"
    } else {
        &name
    };
    let _tenant = tenant_db::create_tenant_with_owner(
        &state.pool,
        &format!("{tenant_name}'s Workspace"),
        None,
        "free",
        user.id,
    )
    .await?;

    let token = create_token(&state.config, user.id)?;

    Ok(Json(AuthResponse {
        token,
        user: user.into(),
    }))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(LoginRequest { email, password }): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, CoreError> {
    let user = users::get_user_by_email(&state.pool, &email)
        .await?
        .ok_or_else(|| CoreError::Unauthorized("invalid credentials".into()))?;

    let parsed_hash = argon2::PasswordHash::new(&user.password_hash)
        .map_err(|_| CoreError::Unauthorized("invalid credentials".into()))?;

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| CoreError::Unauthorized("invalid credentials".into()))?;

    let token = create_token(&state.config, user.id)?;

    Ok(Json(AuthResponse {
        token,
        user: user.into(),
    }))
}

pub fn create_token(config: &AppConfig, user_id: Uuid) -> Result<String, CoreError> {
    let exp = (Utc::now() + Duration::hours(config.jwt_expiry_hours)).timestamp() as usize;
    let claims = Claims { sub: user_id, exp };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;
    Ok(token)
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, CoreError> {
    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));

    let token =
        auth_header.ok_or_else(|| CoreError::Unauthorized("missing bearer token".into()))?;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| CoreError::Unauthorized("invalid token".into()))?;

    let user = users::get_user_by_id(&state.pool, token_data.claims.sub)
        .await
        .map_err(|_| CoreError::Internal("failed to load user".into()))?
        .ok_or_else(|| CoreError::Unauthorized("user no longer exists".into()))?;

    // v1.0: resolve the active tenant from the X-Tenant-ID header (if present),
    // otherwise default to the user's oldest membership. None means the user
    // has no tenants yet (rare — register() creates one).
    let header_tenant_id = request
        .headers()
        .get(&TENANT_HEADER)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok());

    let active_tenant =
        tenant_db::resolve_active_tenant(&state.pool, user.id, header_tenant_id).await?;

    request.extensions_mut().insert(user);
    if let Some(t) = active_tenant {
        request.extensions_mut().insert(t);
    }
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::validate_jwt_secret;

    fn test_config() -> AppConfig {
        AppConfig {
            jwt_secret: "test-secret-that-is-long-enough-for-hmac-000000".into(),
            jwt_expiry_hours: 1,
            storage: credbrew_core::storage::Config {
                s3_endpoint: "http://localhost:9000".into(),
                s3_bucket: "credbrew".into(),
                s3_access_key: "minioadmin".into(),
                s3_secret_key: "minioadmin".into(),
                public_base_url: "http://localhost:8080".into(),
            },
            public_base_url: "http://localhost:8080".into(),
        }
    }

    #[test]
    fn test_token_roundtrip() {
        let config = test_config();
        let id = Uuid::new_v4();
        let token = create_token(&config, id).unwrap();

        let data = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .unwrap();

        assert_eq!(data.claims.sub, id);
    }

    #[test]
    fn test_token_roundtrip_wrong_secret_fails() {
        let config = test_config();
        let token = create_token(&config, Uuid::new_v4()).unwrap();

        let result = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(b"a-different-secret-that-also-is-long-enough-123"),
            &Validation::default(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_jwt_secret_validation() {
        assert!(validate_jwt_secret("short").is_err());
        assert!(validate_jwt_secret("at-least-32-characters-long-secret-value!!").is_ok());
    }
}
