//! credbrew-anchor: background process that batches pending anchoring jobs
//! and submits them to the configured chain.
//!
//! Run separately from the API server and worker:
//!
//! ```bash
//! ETH_RPC_URL=https://polygon-rpc.com \
//! ANCHOR_PRIVATE_KEY=0x... \
//! ANCHOR_WALLET_ADDRESS=0x... \
//! cargo run --bin credbrew-anchor
//! ```
//!
//! Polls anchor_jobs every 30s, batches up to 100 pending jobs per tx,
//! submits, waits for confirmation, records tx_hash + block_number.
//!
//! See: CredBrew v1.0 Design RFC §8.3

use credbrew_core::{
    anchor::{ChainClient, PolygonClient},
    db,
    error::CoreError,
    init_tracing,
};
use sqlx::PgPool;
use std::time::Duration;
use tracing::{error, info};

const POLL_INTERVAL_SECS: u64 = 30;
const BATCH_SIZE: i64 = 100;

#[tokio::main]
async fn main() -> Result<(), CoreError> {
    init_tracing();
    dotenv::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url).await?;

    // Construct the chain client. Phase 6 ships Polygon; the trait allows
    // Arweave to be added later.
    let chain: Box<dyn ChainClient> = Box::new(PolygonClient::from_env()?);
    info!(
        chain = chain.chain_name(),
        poll_interval = POLL_INTERVAL_SECS,
        batch_size = BATCH_SIZE,
        "Anchor process started"
    );

    let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(e) = process_batch(&pool, chain.as_ref()).await {
                    error!("Anchor batch failed: {}", e);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received");
                break;
            }
        }
    }

    info!("Anchor process stopped");
    Ok(())
}

async fn process_batch(pool: &PgPool, chain: &dyn ChainClient) -> Result<(), CoreError> {
    let (jobs, batch_id) =
        db::anchor_jobs::dequeue_batch(pool, chain.chain_name(), BATCH_SIZE).await?;

    if jobs.is_empty() {
        return Ok(());
    }

    info!(
        batch_id,
        count = jobs.len(),
        chain = chain.chain_name(),
        "Anchoring batch"
    );

    let hashes: Vec<String> = jobs.iter().map(|j| j.credential_hash.clone()).collect();

    match chain.anchor_batch(&hashes).await {
        Ok(receipt) => {
            let n = db::anchor_jobs::confirm_batch(
                pool,
                batch_id,
                &receipt.tx_hash,
                receipt.block_number,
            )
            .await?;

            // Also record the anchor on each credential row (for the verify
            // page's "anchored on chain" link).
            for job in &jobs {
                let _ = sqlx::query(
                    r#"
                    UPDATE credentials
                    SET anchored_chain = $2,
                        anchored_txid = $3,
                        anchored_at = now()
                    WHERE id = $1
                    "#,
                )
                .bind(job.credential_id)
                .bind(chain.chain_name())
                .bind(&receipt.tx_hash)
                .execute(pool)
                .await;
            }

            info!(
                batch_id,
                tx_hash = %receipt.tx_hash,
                block_number = receipt.block_number,
                confirmed = n,
                "Batch anchored on {}",
                chain.chain_name()
            );
        }
        Err(e) => {
            let n = db::anchor_jobs::fail_batch(pool, batch_id, &e.to_string()).await?;
            error!(batch_id, failed = n, error = %e, "Batch failed");
        }
    }

    Ok(())
}
