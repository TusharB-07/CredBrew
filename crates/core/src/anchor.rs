//! On-chain anchoring client. Phase 6 implements Polygon PoS via JSON-RPC.
//!
//! The anchor binary batches up to N credential hashes per chain transaction
//! (amortizing gas), submits the tx, waits for confirmation, and records the
//! tx hash + block number on each credential's anchor_jobs row.
//!
//! See: CredBrew v1.0 Design RFC §8

use crate::error::CoreError;
use sha2::{Digest, Sha256};

/// A submitted on-chain anchor transaction.
#[derive(Debug, Clone)]
pub struct AnchorReceipt {
    pub tx_hash: String,
    pub block_number: i64,
}

/// Trait abstraction over chain clients. The anchor binary uses this so we
/// can swap Polygon for Arweave or another chain without touching the binary.
#[async_trait::async_trait]
pub trait ChainClient: Send + Sync {
    /// Submit a batch of credential hashes as a single on-chain transaction.
    /// Returns the tx hash + block number on confirmation.
    async fn anchor_batch(&self, hashes: &[String]) -> Result<AnchorReceipt, CoreError>;

    /// The chain name (e.g. "polygon", "arweave").
    fn chain_name(&self) -> &'static str;

    /// A URL to view the transaction on a block explorer.
    fn explorer_tx_url(&self, tx_hash: &str) -> String;
}

/// Polygon PoS client. Submits the batch hash as calldata to a self-call of
/// the anchor wallet's address — this is the cheapest possible on-chain
/// record (no contract deployment needed; the batch hash is recoverable from
/// the calldata + tx signature).
///
/// Requires:
/// - `ETH_RPC_URL` — a Polygon RPC endpoint (e.g. https://polygon-rpc.com)
/// - `ANCHOR_PRIVATE_KEY` — a funded wallet's private key (hex, no 0x prefix)
///
/// The wallet needs a small amount of POL for gas (~$0.001 per tx at current
/// Polygon fees; one tx per batch of up to 100 credentials).
pub struct PolygonClient {
    rpc_url: String,
    private_key: String,
    wallet_address: String,
    http: reqwest::Client,
}

impl PolygonClient {
    pub fn from_env() -> Result<Self, CoreError> {
        let rpc_url = std::env::var("ETH_RPC_URL")
            .map_err(|_| CoreError::Kms("ETH_RPC_URL must be set for Polygon anchoring".into()))?;
        let private_key = std::env::var("ANCHOR_PRIVATE_KEY").map_err(|_| {
            CoreError::Kms("ANCHOR_PRIVATE_KEY must be set for Polygon anchoring".into())
        })?;
        let wallet_address = derive_address_from_private_key(&private_key)?;
        Ok(Self {
            rpc_url,
            private_key,
            wallet_address,
            http: reqwest::Client::new(),
        })
    }

    /// Get the pending nonce for the wallet address.
    async fn get_nonce(&self) -> Result<u64, CoreError> {
        let resp: serde_json::Value = self
            .http
            .post(&self.rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getTransactionCount",
                "params": [format!("0x{}", self.wallet_address), "pending"],
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| CoreError::Kms(format!("RPC request failed: {e}")))?
            .json()
            .await
            .map_err(|e| CoreError::Kms(format!("RPC response parse failed: {e}")))?;

        let nonce_hex = resp["result"]
            .as_str()
            .ok_or_else(|| CoreError::Kms("missing nonce in RPC response".into()))?;
        u64::from_str_radix(nonce_hex.trim_start_matches("0x"), 16)
            .map_err(|e| CoreError::Kms(format!("invalid nonce: {e}")))
    }

    /// Get the current gas price.
    async fn get_gas_price(&self) -> Result<u64, CoreError> {
        let resp: serde_json::Value = self
            .http
            .post(&self.rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_gasPrice",
                "params": [],
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| CoreError::Kms(format!("RPC request failed: {e}")))?
            .json()
            .await
            .map_err(|e| CoreError::Kms(format!("RPC response parse failed: {e}")))?;

        let gas_hex = resp["result"]
            .as_str()
            .ok_or_else(|| CoreError::Kms("missing gasPrice in RPC response".into()))?;
        u64::from_str_radix(gas_hex.trim_start_matches("0x"), 16)
            .map_err(|e| CoreError::Kms(format!("invalid gasPrice: {e}")))
    }
}

#[async_trait::async_trait]
impl ChainClient for PolygonClient {
    async fn anchor_batch(&self, hashes: &[String]) -> Result<AnchorReceipt, CoreError> {
        if hashes.is_empty() {
            return Err(CoreError::Kms("cannot anchor empty batch".into()));
        }

        // Compute the batch hash: SHA-256 of the concatenation of all
        // credential hashes. This is what gets recorded on-chain.
        let mut hasher = Sha256::new();
        for h in hashes {
            hasher.update(h.as_bytes());
        }
        let batch_hash = hasher.finalize();
        let batch_hash_hex = hex::encode(batch_hash);

        // Build the calldata: just the 32-byte batch hash. This is sent as
        // a self-call (to = from) so no contract is needed. The hash is
        // recoverable from the calldata by anyone reading the chain.
        let calldata = format!("0x{}", batch_hash_hex);

        let nonce = self.get_nonce().await?;
        let gas_price = self.get_gas_price().await?;
        let gas_limit: u64 = 30_000; // self-call with 32 bytes calldata is cheap

        // Construct and sign the raw transaction. This is a simplified
        // legacy (type-0) tx for Polygon PoS. A production impl should use
        // ethers-rs or alloy for proper EIP-1559 / type-2 tx support.
        let signed_tx = sign_legacy_tx(
            &self.private_key,
            &self.wallet_address, // self-call
            &calldata,
            nonce,
            gas_price,
            gas_limit,
            137, // Polygon chainId
        )?;

        // Submit the signed tx.
        let resp: serde_json::Value = self
            .http
            .post(&self.rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_sendRawTransaction",
                "params": [format!("0x{}", signed_tx)],
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| CoreError::Kms(format!("RPC submit failed: {e}")))?
            .json()
            .await
            .map_err(|e| CoreError::Kms(format!("RPC submit parse failed: {e}")))?;

        let tx_hash = resp["result"]
            .as_str()
            .ok_or_else(|| {
                CoreError::Kms(format!(
                    "tx submission failed: {}",
                    resp["error"]["message"].as_str().unwrap_or("unknown")
                ))
            })?
            .to_string();

        // Poll for the receipt (block number). Polygon blocks are ~2s.
        let block_number = poll_for_receipt(&self.http, &self.rpc_url, &tx_hash).await?;

        Ok(AnchorReceipt {
            tx_hash,
            block_number,
        })
    }

    fn chain_name(&self) -> &'static str {
        "polygon"
    }

    fn explorer_tx_url(&self, tx_hash: &str) -> String {
        format!("https://polygonscan.com/tx/{}", tx_hash)
    }
}

/// Derive the Ethereum address from a private key (secp256k1).
/// Note: this requires the `secp256k1` crate. For Phase 6 we stub this —
/// the anchor binary is structurally complete but actual signing requires
/// a real crypto library. The wallet address should be set explicitly via
/// `ANCHOR_WALLET_ADDRESS` env var until the full signing path is wired.
fn derive_address_from_private_key(_private_key: &str) -> Result<String, CoreError> {
    std::env::var("ANCHOR_WALLET_ADDRESS").map_err(|_| {
        CoreError::Kms(
            "ANCHOR_WALLET_ADDRESS must be set (derivation from private key not yet implemented; \
             use a real ETH wallet to get the address)"
                .into(),
        )
    })
}

/// Sign a legacy (type-0) transaction. Stub for now — requires secp256k1
/// + RLP encoding. A production impl should use ethers-rs or alloy.
///
/// Returns the hex-encoded signed tx (without 0x prefix).
fn sign_legacy_tx(
    _private_key: &str,
    _to: &str,
    _calldata: &str,
    _nonce: u64,
    _gas_price: u64,
    _gas_limit: u64,
    _chain_id: u64,
) -> Result<String, CoreError> {
    Err(CoreError::Kms(
        "transaction signing not yet implemented — integrate ethers-rs or alloy to enable live anchoring. \
         The anchor binary structure, batching, DB integration, and confirm/fail flows are complete; \
         only the raw tx signing + RLP encoding is stubbed."
            .into(),
    ))
}

/// Poll `eth_getTransactionReceipt` until the tx is mined.
async fn poll_for_receipt(
    http: &reqwest::Client,
    rpc_url: &str,
    tx_hash: &str,
) -> Result<i64, CoreError> {
    for _ in 0..30 {
        // up to ~60s (30 polls × 2s)
        let resp: serde_json::Value = http
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getTransactionReceipt",
                "params": [tx_hash],
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| CoreError::Kms(format!("receipt poll failed: {e}")))?
            .json()
            .await
            .map_err(|e| CoreError::Kms(format!("receipt parse failed: {e}")))?;

        if let Some(result) = resp["result"].as_object() {
            if let Some(block_hex) = result["blockNumber"].as_str() {
                return u64::from_str_radix(block_hex.trim_start_matches("0x"), 16)
                    .map(|b| b as i64)
                    .map_err(|e| CoreError::Kms(format!("invalid blockNumber: {e}")));
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Err(CoreError::Kms(
        "tx not mined within 60s; check the chain manually".into(),
    ))
}
