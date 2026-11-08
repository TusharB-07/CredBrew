//! Cryptographic key storage: a KeyStore trait with two implementations.
//!
//! Phase 2 ships:
//! - `LocalFileKeyStore`: a development-grade impl that XOR-seals the master
//!   key with a passphrase read from the `CREDBREW_MASTER_KEY` env var.
//!   Suitable for local dev and self-hosted single-tenant deployments.
//!
//! Future phases will add:
//! - `AwsKmsKeyStore`: production-grade, AWS KMS-backed.
//!
//! The KeyStore seals/unseals the 32-byte Ed25519 seed before it touches the
//! database. The master key never leaves the KeyStore; the database stores
//! only the sealed bytes.
//!
//! See: CredBrew v1.0 Design RFC §4.4

use crate::error::CoreError;
use sha2::{Digest, Sha256};

/// A key store: produces sealed bytes for DB storage and unseals them on demand.
pub trait KeyStore: Send + Sync {
    /// Seal a 32-byte Ed25519 seed. Returns bytes safe to persist.
    fn seal(&self, seed: &[u8; 32]) -> Result<Vec<u8>, CoreError>;

    /// Unseal a previously-sealed seed back to 32 bytes.
    fn unseal(&self, sealed: &[u8]) -> Result<[u8; 32], CoreError>;
}

/// A simple XOR-based key store keyed off a passphrase from the
/// `CREDBREW_MASTER_KEY` environment variable.
///
/// The 32-byte key is derived as SHA-256(passphrase). The sealed form is
/// XOR(seed, key) — sufficient to keep the seed out of plaintext on disk,
/// but NOT a substitute for real KMS in production. Documented accordingly.
pub struct LocalFileKeyStore {
    key: [u8; 32],
}

impl LocalFileKeyStore {
    pub fn from_env() -> Result<Self, CoreError> {
        let passphrase = std::env::var("CREDBREW_MASTER_KEY")
            .map_err(|_| CoreError::Kms("CREDBREW_MASTER_KEY env var must be set".into()))?;
        if passphrase.len() < 16 {
            return Err(CoreError::Kms(
                "CREDBREW_MASTER_KEY must be at least 16 characters".into(),
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(passphrase.as_bytes());
        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash);
        Ok(Self { key })
    }

    /// Construct from a passphrase. Also available outside `cfg(test)` so
    /// integration tests (which are a separate crate) can use it.
    pub fn from_passphrase(passphrase: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(passphrase.as_bytes());
        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash);
        Self { key }
    }
}

impl KeyStore for LocalFileKeyStore {
    fn seal(&self, seed: &[u8; 32]) -> Result<Vec<u8>, CoreError> {
        // XOR the seed with the derived key. Simple and reversible.
        // The first byte is a version tag (0x01) so future formats can
        // upgrade without ambiguity.
        let mut out = Vec::with_capacity(33);
        out.push(0x01);
        for (i, b) in seed.iter().enumerate() {
            out.push(b ^ self.key[i % 32]);
        }
        Ok(out)
    }

    fn unseal(&self, sealed: &[u8]) -> Result<[u8; 32], CoreError> {
        if sealed.len() != 33 {
            return Err(CoreError::Kms(format!(
                "sealed seed must be 33 bytes (1 version + 32 data), got {}",
                sealed.len()
            )));
        }
        if sealed[0] != 0x01 {
            return Err(CoreError::Kms(format!(
                "unknown sealed seed version: 0x{:02x}",
                sealed[0]
            )));
        }
        let mut out = [0u8; 32];
        for (i, b) in sealed[1..].iter().enumerate() {
            out[i] = b ^ self.key[i % 32];
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seal_unseal_roundtrip() {
        let ks = LocalFileKeyStore::from_passphrase("test-passphrase-long-enough");
        let seed = [42u8; 32];
        let sealed = ks.seal(&seed).unwrap();
        assert_eq!(sealed.len(), 33);
        assert_eq!(sealed[0], 0x01); // version tag
        let unsealed = ks.unseal(&sealed).unwrap();
        assert_eq!(unsealed, seed);
    }

    #[test]
    fn test_sealed_differs_from_plaintext() {
        let ks = LocalFileKeyStore::from_passphrase("test-passphrase-long-enough");
        let seed = [0xAA; 32];
        let sealed = ks.seal(&seed).unwrap();
        // The sealed payload must not contain the raw seed bytes.
        assert!(!sealed[1..].iter().all(|&b| b == 0xAA));
    }

    #[test]
    fn test_wrong_passphrase_fails_to_unseal() {
        let ks1 = LocalFileKeyStore::from_passphrase("passphrase-one-long-enough");
        let ks2 = LocalFileKeyStore::from_passphrase("passphrase-two-long-enough");
        let seed = [7u8; 32];
        let sealed = ks1.seal(&seed).unwrap();
        let unsealed_with_wrong = ks2.unseal(&sealed).unwrap();
        assert_ne!(
            unsealed_with_wrong, seed,
            "wrong passphrase must produce different bytes"
        );
    }

    #[test]
    fn test_corrupted_sealed_fails() {
        let ks = LocalFileKeyStore::from_passphrase("test-passphrase-long-enough");
        let mut sealed = ks.seal(&[1u8; 32]).unwrap();
        sealed[0] = 0x99; // wrong version
        assert!(ks.unseal(&sealed).is_err());
    }

    #[test]
    fn test_wrong_length_fails() {
        let ks = LocalFileKeyStore::from_passphrase("test-passphrase-long-enough");
        assert!(ks.unseal(&[0x01, 0x02]).is_err());
    }
}
