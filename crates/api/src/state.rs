use credbrew_core::{
    crypto::KeyStore,
    error::CoreError,
    storage::{Config as StorageConfig, StorageClient},
};
use sqlx::PgPool;
use std::sync::Arc;

pub struct AppState {
    pub pool: PgPool,
    pub config: AppConfig,
    pub storage: StorageClient,
    pub keystore: Arc<dyn KeyStore>,
}

#[derive(Clone)]
pub struct AppConfig {
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub storage: StorageConfig,
    pub public_base_url: String,
}

const KNOWN_DEV_JWT_SECRET: &str = "change-me-64-chars-minimum-length-for-security";

pub fn validate_jwt_secret(secret: &str) -> Result<(), CoreError> {
    if secret.len() < 32 {
        return Err(CoreError::Config(config::ConfigError::Message(
            "JWT_SECRET must be at least 32 characters long".into(),
        )));
    }
    if secret == KNOWN_DEV_JWT_SECRET {
        tracing::warn!(
            "JWT_SECRET is set to the known development value from .env.example; use a unique value outside local dev"
        );
    }
    Ok(())
}

impl AppConfig {
    pub fn from_env() -> Result<Self, CoreError> {
        let jwt_secret = std::env::var("JWT_SECRET")?;
        validate_jwt_secret(&jwt_secret)?;

        let storage = StorageConfig::from_env()?;

        Ok(Self {
            public_base_url: std::env::var("PUBLIC_BASE_URL")?,
            jwt_expiry_hours: std::env::var("JWT_EXPIRY_HOURS")
                .ok()
                .map(|v| {
                    v.parse::<i64>().map_err(|_| {
                        CoreError::Config(config::ConfigError::Message(
                            "JWT_EXPIRY_HOURS must be a number".into(),
                        ))
                    })
                })
                .transpose()?
                .unwrap_or(24),
            jwt_secret,
            storage,
        })
    }
}
