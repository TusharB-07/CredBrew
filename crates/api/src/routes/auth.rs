use std::ops::Deref;
use std::sync::Arc;
use axum::{
    extract::{State, Request, FromRequestParts},
    middleware::Next,
    response::Response,
    http::request::Parts,
    Json,
};
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};
use argon2::{PasswordHasher, PasswordVerifier, Argon2};
use argon2::password_hash::{SaltString, rand_core::OsRng};
use uuid::Uuid;
use chrono::{Utc, Duration};
use serde::Deserialize;
use crate::state::{AppState, AppConfig};
use credbrew_core::{
    db::users,
    models::{User, Claims, AuthResponse},
    error::CoreError,
};

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
pub struct AuthUser(pub User);

impl Deref for AuthUser {
    type Target = User;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = CoreError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<User>()
            .cloned()
            .map(AuthUser)
            .ok_or_else(|| CoreError::Unauthorized("missing or invalid auth token".into()))
    }
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(RegisterRequest { name, email, password }): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, CoreError> {
    if password.len() < 8 {
        return Err(CoreError::TemplateValidation("password must be at least 8 characters".into()));
    }

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2.hash_password(password.as_bytes(), &salt)?.to_string();

    let user = users::create_user(&state.pool, &email, &password_hash, &name).await?;

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
    let user = users::get_user_by_email(&state.pool, &email).await?
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
    let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(config.jwt_secret.as_bytes()))?;
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

    let token = auth_header.ok_or_else(|| CoreError::Unauthorized("missing bearer token".into()))?;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| CoreError::Unauthorized("invalid token".into()))?;

    let user = users::get_user_by_id(&state.pool, token_data.claims.sub).await
        .map_err(|_| CoreError::Internal("failed to load user".into()))?
        .ok_or_else(|| CoreError::Unauthorized("user no longer exists".into()))?;

    request.extensions_mut().insert(user);
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