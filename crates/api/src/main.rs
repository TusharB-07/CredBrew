use axum::{
    middleware,
    response::Json,
    routing::{delete, get, patch, post},
    Router,
};
use credbrew_core::{
    crypto::LocalFileKeyStore, error::CoreError, init_tracing, storage::StorageClient,
};
use sqlx::PgPool;
use std::sync::Arc;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::GovernorLayer;
use tracing::info;

mod routes;
mod state;

#[axum::debug_handler]
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[tokio::main]
async fn main() -> Result<(), CoreError> {
    dotenv::dotenv().ok();
    init_tracing();

    let config = state::AppConfig::from_env()?;
    let pool = PgPool::connect(&std::env::var("DATABASE_URL")?).await?;

    sqlx::migrate!("../../migrations").run(&pool).await?;

    let storage = StorageClient::new(&config.storage)?;

    // v1.0 Phase 2: load the KMS-backed keystore (LocalFileKeyStore for dev;
    // AWS KMS impl lands in a later phase). The keystore seals/unseals the
    // 32-byte Ed25519 seeds before they touch the database.
    let keystore: Arc<dyn credbrew_core::crypto::KeyStore> =
        Arc::new(LocalFileKeyStore::from_env()?);

    let state = Arc::new(state::AppState {
        pool,
        config,
        storage,
        keystore,
    });

    let rate_limiting = GovernorLayer {
        config: Arc::new(
            GovernorConfigBuilder::default()
                .per_second(5)
                .burst_size(10)
                .finish()
                .unwrap(),
        ),
    };
    let auth_limiter = GovernorLayer {
        config: Arc::new(
            GovernorConfigBuilder::default()
                .per_second(5)
                .burst_size(5)
                .finish()
                .unwrap(),
        ),
    };
    let public_limiter = GovernorLayer {
        config: Arc::new(
            GovernorConfigBuilder::default()
                .per_second(30)
                .burst_size(30)
                .finish()
                .unwrap(),
        ),
    };

    let public_routes = Router::new()
        .route("/health", get(health))
        .route(
            "/api/auth/register",
            post(routes::auth::register).layer(auth_limiter.clone()),
        )
        .route(
            "/api/auth/login",
            post(routes::auth::login).layer(auth_limiter),
        )
        .route(
            "/api/public/verify/{code}",
            get(routes::public::verify_credential).layer(public_limiter.clone()),
        )
        .route(
            "/api/public/verify/asset/{code}",
            get(routes::public::credential_asset),
        )
        .route(
            "/api/public/og-image/{code}",
            get(routes::public::og_image).layer(public_limiter),
        )
        .route("/v/{code}", get(routes::public::verify_page))
        // v1.0 Phase 2: public DID document endpoint (serves did.json for did:web resolution)
        .route(
            "/tenants/{slug}/did.json",
            get(routes::dids::serve_tenant_did_document),
        )
        .route(
            "/.well-known/did.json",
            get(routes::dids::serve_root_did_document),
        );

    let protected_routes = Router::new()
        .route(
            "/api/events",
            get(routes::events::list_events).post(routes::events::create_event),
        )
        .route(
            "/api/events/{id}",
            get(routes::events::get_event).patch(routes::events::update_event),
        )
        .route(
            "/api/events/{id}/types",
            post(routes::participant_types::create_type),
        )
        .route(
            "/api/types/{id}",
            patch(routes::participant_types::update_type)
                .delete(routes::participant_types::delete_type),
        )
        .route(
            "/api/events/{id}/participants",
            get(routes::participants::list_participants)
                .post(routes::participants::create_participant),
        )
        .route(
            "/api/events/{id}/participants/import",
            post(routes::participants::import_participants),
        )
        .route(
            "/api/participants/{id}",
            delete(routes::participants::delete_participant),
        )
        .route("/api/templates", post(routes::templates::create_template))
        .route(
            "/api/templates/{id}/preview",
            get(routes::templates::preview_template),
        )
        .route(
            "/api/templates/{id}/file",
            get(routes::templates::get_template_file),
        )
        .route(
            "/api/files/templates/{file_key}",
            get(routes::files::download_template),
        )
        .route(
            "/api/files/credentials/{credential_id}/download",
            get(routes::files::download_credential),
        )
        .route(
            "/api/events/{id}/issue",
            post(routes::issue::issue_credentials),
        )
        .route(
            "/api/events/{id}/issue-status",
            get(routes::issue::issue_status),
        )
        .route(
            "/api/credentials/{id}/retry",
            post(routes::issue::retry_credential),
        )
        .route(
            "/api/credentials/{id}",
            patch(routes::issue::revoke_credential),
        )
        // v1.0 Phase 2: DID + signing key management
        .route(
            "/api/tenants/me/did",
            get(routes::dids::get_active_did).post(routes::dids::mint_did),
        )
        .route("/api/tenants/me/keys", get(routes::dids::list_signing_keys))
        .route(
            "/api/tenants/me/keys/{id}/retire",
            post(routes::dids::retire_key),
        )
        .route(
            "/api/tenants/me/keys/{id}/revoke",
            post(routes::dids::revoke_key),
        )
        .route("/api/tenants/me/sign-test", post(routes::dids::sign_test))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            routes::auth::auth_middleware,
        ))
        .layer(rate_limiting);

    let app = public_routes.merge(protected_routes).with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("Server running on http://0.0.0.0:8080");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
