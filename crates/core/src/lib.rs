pub mod db;
pub mod error;
pub mod models;
pub mod queue;
pub mod render;
pub mod short_code;
pub mod storage;

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .compact()
        .init();
}