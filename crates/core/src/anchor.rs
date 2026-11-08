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
        // Derive the wallet address from the private key (no separate env var needed).
        let wallet_address = derive_address_from_private_key(&private_key)?;
        tracing::info!(
            wallet_address = %wallet_address,
            "Anchor wallet address derived from private key"
        );
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

/// Derive the Ethereum address from a secp256k1 private key.
/// The address is the last 20 bytes of keccak256(public_key_bytes).
fn derive_address_from_private_key(private_key_hex: &str) -> Result<String, CoreError> {
    let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
        .map_err(|e| CoreError::Kms(format!("invalid private key hex: {e}")))?;
    if pk_bytes.len() != 32 {
        return Err(CoreError::Kms(format!(
            "private key must be 32 bytes, got {}",
            pk_bytes.len()
        )));
    }

    let secret_key = secp256k1::SecretKey::from_slice(&pk_bytes)
        .map_err(|e| CoreError::Kms(format!("invalid secp256k1 key: {e}")))?;
    let public_key = secp256k1::PublicKey::from_secret_key(secp256k1::SECP256K1, &secret_key);

    // Serialize uncompressed (65 bytes: 0x04 + 32 X + 32 Y), drop the 0x04 prefix
    let serialized = public_key.serialize_uncompressed();
    let pub_bytes = &serialized[1..]; // 64 bytes

    // keccak256(pub_bytes), take last 20 bytes
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(pub_bytes);
    let hash = hasher.finalize();
    let address = &hash[12..]; // last 20 bytes
    Ok(hex::encode(address))
}

/// Sign a legacy (type-0) EIP-155 transaction using secp256k1 + keccak256 + RLP.
///
/// The signed tx is RLP-encoded as:
/// [nonce, gas_price, gas_limit, to, value, data, v, r, s]
///
/// The signing hash (for EIP-155) is keccak256(RLP([nonce, gas_price, gas_limit, to, value, data, chain_id, 0, 0]))
///
/// Returns the hex-encoded signed tx (without 0x prefix).
fn sign_legacy_tx(
    private_key_hex: &str,
    to_hex: &str,
    calldata_hex: &str,
    nonce: u64,
    gas_price: u64,
    gas_limit: u64,
    chain_id: u64,
) -> Result<String, CoreError> {
    // Parse inputs
    let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
        .map_err(|e| CoreError::Kms(format!("invalid private key hex: {e}")))?;
    let secret_key = secp256k1::SecretKey::from_slice(&pk_bytes)
        .map_err(|e| CoreError::Kms(format!("invalid secp256k1 key: {e}")))?;

    let to_bytes = hex::decode(to_hex.trim_start_matches("0x"))
        .map_err(|e| CoreError::Kms(format!("invalid to address: {e}")))?;
    let data_bytes = hex::decode(calldata_hex.trim_start_matches("0x"))
        .map_err(|e| CoreError::Kms(format!("invalid calldata: {e}")))?;

    // Step 1: Build the EIP-155 signing hash
    // RLP([nonce, gas_price, gas_limit, to, value, data, chain_id, 0, 0])
    let signing_rlp = rlp_encode_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_u64(gas_price),
        rlp_encode_u64(gas_limit),
        rlp_encode_bytes(&to_bytes),
        rlp_encode_bytes(&[]), // value = 0 ETH
        rlp_encode_bytes(&data_bytes),
        rlp_encode_u64(chain_id),
        rlp_encode_bytes(&[]), // 0
        rlp_encode_bytes(&[]), // 0
    ]);

    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(&signing_rlp);
    let signing_hash = hasher.finalize();

    // Step 2: Sign with secp256k1
    let msg = secp256k1::Message::from_digest_slice(&signing_hash)
        .map_err(|e| CoreError::Kms(format!("invalid message hash: {e}")))?;
    let sig = secp256k1::SECP256K1.sign_ecdsa_recoverable(&msg, &secret_key);
    let (recovery_id, sig_bytes) = sig.serialize_compact();

    let r = &sig_bytes[0..32];
    let s = &sig_bytes[32..64];

    // EIP-155: v = chain_id * 2 + 35 + recovery_id
    let v = chain_id * 2 + 35 + recovery_id.to_i32() as u64;

    // Step 3: Build the final signed tx RLP
    // RLP([nonce, gas_price, gas_limit, to, value, data, v, r, s])
    let signed_rlp = rlp_encode_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_u64(gas_price),
        rlp_encode_u64(gas_limit),
        rlp_encode_bytes(&to_bytes),
        rlp_encode_bytes(&[]), // value
        rlp_encode_bytes(&data_bytes),
        rlp_encode_u64(v),
        rlp_encode_bytes(r),
        rlp_encode_bytes(s),
    ]);

    Ok(hex::encode(&signed_rlp))
}

// ─────────────────────────────────────────────────────────────────────────────
// Minimal RLP (Recursive Length Prefix) encoding per Ethereum yellow paper.
// ─────────────────────────────────────────────────────────────────────────────

/// RLP-encode a byte string.
fn rlp_encode_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() == 1 && bytes[0] < 0x80 {
        return bytes.to_vec();
    }
    let mut out = rlp_length_prefix(bytes.len(), 0x80);
    out.extend_from_slice(bytes);
    out
}

/// RLP-encode a u64 as a big-endian byte string (stripping leading zeros).
fn rlp_encode_u64(val: u64) -> Vec<u8> {
    if val == 0 {
        return rlp_encode_bytes(&[]); // 0x80
    }
    let bytes = val.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(8);
    rlp_encode_bytes(&bytes[start..])
}

/// RLP-encode a list of pre-encoded items.
fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = Vec::new();
    for item in items {
        payload.extend_from_slice(item);
    }
    let mut out = rlp_length_prefix(payload.len(), 0xc0);
    out.extend_from_slice(&payload);
    out
}

/// Compute the RLP length prefix for a payload of `data_len` bytes.
/// `offset` is 0x80 for byte strings, 0xc0 for lists.
fn rlp_length_prefix(data_len: usize, offset: u8) -> Vec<u8> {
    if data_len < 56 {
        vec![offset + data_len as u8]
    } else {
        let len_bytes = strip_leading_zeros(&(data_len as u64).to_be_bytes());
        let mut out = vec![offset + 55 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out
    }
}

fn strip_leading_zeros(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    bytes[start..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlp_encode_empty_bytes() {
        assert_eq!(rlp_encode_bytes(&[]), vec![0x80]);
    }

    #[test]
    fn test_rlp_encode_single_byte() {
        assert_eq!(rlp_encode_bytes(&[0x0a]), vec![0x0a]); // < 0x80, just the byte
        assert_eq!(rlp_encode_bytes(&[0x80]), vec![0x81, 0x80]); // >= 0x80, needs prefix
    }

    #[test]
    fn test_rlp_encode_short_string() {
        assert_eq!(rlp_encode_bytes(b"dog"), vec![0x83, b'd', b'o', b'g']);
    }

    #[test]
    fn test_rlp_encode_u64() {
        assert_eq!(rlp_encode_u64(0), vec![0x80]);
        assert_eq!(rlp_encode_u64(1), vec![0x01]);
        assert_eq!(rlp_encode_u64(1024), vec![0x82, 0x04, 0x00]);
    }

    #[test]
    fn test_rlp_encode_list() {
        let items = vec![rlp_encode_bytes(b"cat"), rlp_encode_bytes(b"dog")];
        let encoded = rlp_encode_list(&items);
        // [0xc8, 0x83, 'c', 'a', 't', 0x83, 'd', 'o', 'g']
        assert_eq!(
            encoded,
            vec![0xc8, 0x83, b'c', b'a', b't', 0x83, b'd', b'o', b'g']
        );
    }

    #[test]
    fn test_derive_address_well_known_key() {
        // Well-known test key: private key 1 → address 0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf
        let addr = derive_address_from_private_key(
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap();
        assert_eq!(addr, "7e5f4552091a69125d5dfcb7b8c2659029395bdf");
    }

    #[test]
    fn test_sign_legacy_tx_produces_valid_output() {
        // Sign a simple tx and verify the output is non-empty and starts with
        // a valid RLP list prefix. We can't verify the exact signature without
        // a reference implementation, but we can check structural validity.
        let signed = sign_legacy_tx(
            "0000000000000000000000000000000000000000000000000000000000000001",
            "7e5f4552091a69125d5dfcb7b8c2659029395bdf", // self-call
            "deadbeef",
            0,
            1_000_000_000, // 1 gwei
            30_000,
            137, // Polygon
        )
        .unwrap();

        // The signed tx should be a valid hex string
        let bytes = hex::decode(&signed).unwrap();
        // First byte should be 0xc0+ (RLP list prefix)
        assert!(
            bytes[0] >= 0xc0,
            "first byte should be RLP list prefix, got 0x{:02x}",
            bytes[0]
        );
        // Should be reasonably long (9 items in the list)
        assert!(
            bytes.len() > 50,
            "signed tx too short: {} bytes",
            bytes.len()
        );
    }
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
