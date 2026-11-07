//! DID (Decentralized Identifier) utilities for CredBrew v1.0.
//!
//! Phase 2 supports the `did:web` method exclusively. The trust root is DNS +
//! TLS — the issuer's DID document is served at `https://{domain}/.well-known/did.json`
//! (for root-domain DIDs) or `https://{domain}/{path}/did.json` (for sub-path DIDs).
//!
//! See: <https://w3c-ccg.github.io/did-method-web/>
//! See: CredBrew v1.0 Design RFC §4

use crate::error::CoreError;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use multibase::Base;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// A freshly-generated Ed25519 keypair, before persistence.
pub struct FreshKeypair {
    pub seed: [u8; 32],
    pub public_key: VerifyingKey,
    pub public_key_multibase: String,
}

/// Generate a new Ed25519 keypair using the OS CSPRNG.
pub fn generate_keypair() -> FreshKeypair {
    let mut csprng = OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verifying: VerifyingKey = signing.verifying_key();
    let seed = signing.to_bytes();
    let public_key_multibase = encode_public_key_multibase(&verifying);
    FreshKeypair {
        seed,
        public_key: verifying,
        public_key_multibase,
    }
}

/// Encode an Ed25519 public key as a multibase string (the `z` prefix +
/// base58btc encoding of the 32-byte raw public key, per the
/// Ed25519VerificationKey2018 spec).
pub fn encode_public_key_multibase(pk: &VerifyingKey) -> String {
    let bytes = pk.to_bytes();
    // Multibase 'z' = base58btc
    multibase::encode(Base::Base58Btc, &bytes[..])
}

/// Decode a multibase-encoded public key back into a VerifyingKey.
pub fn decode_public_key_multibase(s: &str) -> Result<VerifyingKey, CoreError> {
    let (_base, bytes) = multibase::decode(s)
        .map_err(|e| CoreError::Did(format!("invalid multibase public key: {e}")))?;
    if bytes.len() != 32 {
        return Err(CoreError::Did(format!(
            "public key must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&arr).map_err(|e| CoreError::Did(format!("invalid ed25519 key: {e}")))
}

/// Reconstruct an Ed25519 SigningKey from a 32-byte seed.
pub fn signing_key_from_seed(seed: &[u8; 32]) -> SigningKey {
    SigningKey::from_bytes(seed)
}

// ─────────────────────────────────────────────────────────────────────────────
// did:web
// ─────────────────────────────────────────────────────────────────────────────

/// Build the did:web identifier for a tenant.
///
/// - Root-domain form: `did:web:org.example` → resolves at `https://org.example/.well-known/did.json`
/// - Sub-path form:    `did:web:credbrew.example:tenants:techconf` → resolves at
///   `https://credbrew.example/tenants/techconf/did.json`
///
/// The `path_segments` are URL-path components joined with `:` in the DID
/// (per the did:web spec, `:` replaces `/`).
pub fn build_did_web(domain: &str, path_segments: &[&str]) -> String {
    let mut parts: Vec<&str> = vec![domain];
    parts.extend_from_slice(path_segments);
    format!("did:web:{}", parts.join(":"))
}

/// The URL where the DID document for a did:web identifier is served.
pub fn did_web_document_url(domain: &str, path_segments: &[&str]) -> String {
    if path_segments.is_empty() {
        format!("https://{domain}/.well-known/did.json")
    } else {
        let path = path_segments.join("/");
        format!("https://{domain}/{path}/did.json")
    }
}

/// Build a minimal W3C-conformant DID document for a did:web identifier
/// with a single Ed25519VerificationKey2018 verification method.
///
/// The document shape is what gets served at the did.json URL and what
/// verifiers fetch to obtain the issuer's public key.
pub fn build_did_document(did: &str, key_id_fragment: &str, public_key_multibase: &str) -> Value {
    let vm_id = format!("{did}#{key_id_fragment}");
    json!({
        "@context": [
            "https://www.w3.org/ns/did/v1",
            "https://w3id.org/security/suites/ed25519-2020/v1"
        ],
        "id": did,
        "verificationMethod": [{
            "id": vm_id,
            "type": "Ed25519VerificationKey2020",
            "controller": did,
            "publicKeyMultibase": public_key_multibase
        }],
        "authentication": [vm_id.clone()],
        "assertionMethod": [vm_id]
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// LD-Proof (Ed25519Signature2020) signing and verification
//
// This is a minimal, spec-compliant implementation of the W3C VC Data
// Integrity Ed25519Signature2020 suite. It does NOT use the full JSON-LD
// canonicalization (URDNA2015) machinery — instead it canonicalizes via a
// deterministic JSON serialization (BONED — fields sorted, no whitespace).
// This is sufficient for CredBrew's internal use where both signer and
// verifier agree on the canonicalization. For interoperability with
// third-party wallets that demand strict URDNA2015, a future PR can swap
// in the `rdf-canon` crate without changing the on-disk format (the
// proofValue is just an Ed25519 signature over the canonicalized bytes).
// ─────────────────────────────────────────────────────────────────────────────

/// The proof block attached to a Verifiable Credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    #[serde(rename = "type")]
    pub proof_type: String,
    #[serde(rename = "created")]
    pub created: String,
    #[serde(rename = "verificationMethod")]
    pub verification_method: String,
    #[serde(rename = "proofPurpose")]
    pub proof_purpose: String,
    #[serde(rename = "proofValue", skip_serializing_if = "Option::is_none")]
    pub proof_value: Option<String>,
}

impl Proof {
    /// Build an Ed25519Signature2020 proof block (without the proofValue,
    /// which is computed by `sign_document`).
    pub fn ed25519_signature_2020(
        verification_method: &str,
        created: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Proof {
            proof_type: "Ed25519Signature2020".to_string(),
            created: created.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            verification_method: verification_method.to_string(),
            proof_purpose: "assertionMethod".to_string(),
            proof_value: None,
        }
    }
}

/// Canonicalize a JSON value for signing. Deterministic serialization:
/// - Object keys sorted alphabetically
/// - No whitespace
/// - UTF-8 output
///
/// This is NOT RFC 8785 (JSON Canonicalization Scheme) and NOT URDNA2015,
/// but it is deterministic and sufficient for our internal sign+verify
/// loop. See module docs for the upgrade path.
fn canonicalize(value: &Value) -> Result<Vec<u8>, CoreError> {
    canonicalize_value(value)
}

fn canonicalize_value(v: &Value) -> Result<Vec<u8>, CoreError> {
    let mut out = Vec::new();
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(serialize_string(k).as_bytes());
                out.push(b':');
                out.extend_from_slice(&canonicalize_value(&map[*k])?);
            }
            out.push(b'}');
        }
        Value::Array(arr) => {
            out.push(b'[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(&canonicalize_value(item)?);
            }
            out.push(b']');
        }
        Value::String(s) => {
            out.extend_from_slice(serialize_string(s).as_bytes());
        }
        Value::Number(n) => {
            out.extend_from_slice(n.to_string().as_bytes());
        }
        Value::Bool(b) => {
            out.extend_from_slice(if *b { b"true" } else { b"false" });
        }
        Value::Null => {
            out.extend_from_slice(b"null");
        }
    }
    Ok(out)
}

fn serialize_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Hash the canonicalized document with SHA-256, then sign with Ed25519.
/// Returns the proof block and the signed document (with the proof block,
/// including proofValue, embedded).
pub fn sign_document(
    document: &Value,
    verification_method: &str,
    signing_key: &SigningKey,
) -> Result<(Proof, Value), CoreError> {
    let created = chrono::Utc::now();
    let proof = Proof::ed25519_signature_2020(verification_method, created);

    // Build the unsigned document with the proof block (sans proofValue).
    // The proofValue is omitted during signing; it's added after.
    let mut doc_for_signing = document.clone();
    if let Value::Object(ref mut map) = doc_for_signing {
        map.insert(
            "proof".to_string(),
            serde_json::to_value(&proof).map_err(|e| CoreError::Signing(e.to_string()))?,
        );
    }

    // Canonicalize + hash + sign.
    let canonical = canonicalize(&doc_for_signing)?;
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    let hash = hasher.finalize();
    let signature: ed25519_dalek::Signature = signing_key.sign(&hash);
    let sig_bytes = signature.to_bytes();

    // Encode as multibase base58btc with 'z' prefix.
    let proof_value = multibase::encode(Base::Base58Btc, &sig_bytes[..]);
    let mut signed_proof = proof;
    signed_proof.proof_value = Some(proof_value);

    // Re-insert the now-complete proof block into the document.
    let mut signed_doc = doc_for_signing;
    if let Value::Object(ref mut map) = signed_doc {
        map.insert(
            "proof".to_string(),
            serde_json::to_value(&signed_proof).map_err(|e| CoreError::Signing(e.to_string()))?,
        );
    }

    Ok((signed_proof, signed_doc))
}

/// Verify a Verifiable Credential's Ed25519Signature2020 proof.
///
/// `document` must include the `proof` block with a `proofValue`. The
/// `verification_method` field in the proof must be a DID URL that resolves
/// (via the DID document) to a public key. The caller is responsible for
/// fetching the DID document; this function takes the public key directly.
pub fn verify_document(document: &Value, public_key: &VerifyingKey) -> Result<bool, CoreError> {
    let proof_value = document
        .get("proof")
        .and_then(|p| p.get("proofValue"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Verification("missing proof.proofValue".into()))?;

    // Reconstruct the unsigned-with-proof document (clone, remove proofValue).
    let mut doc_for_verify = document.clone();
    if let Some(proof) = doc_for_verify
        .get_mut("proof")
        .and_then(|p| p.as_object_mut())
    {
        proof.remove("proofValue");
    }

    let canonical = canonicalize(&doc_for_verify)?;
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    let hash = hasher.finalize();

    let (_base, sig_bytes) = multibase::decode(proof_value)
        .map_err(|e| CoreError::Verification(format!("invalid proofValue encoding: {e}")))?;
    if sig_bytes.len() != 64 {
        return Err(CoreError::Verification(format!(
            "signature must be 64 bytes, got {}",
            sig_bytes.len()
        )));
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&sig_bytes);
    let signature = ed25519_dalek::Signature::from_bytes(&arr);

    Ok(public_key.verify(&hash, &signature).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation_and_encoding() {
        let kp = generate_keypair();
        assert_eq!(kp.seed.len(), 32);
        assert!(kp.public_key_multibase.starts_with('z'));
        // Roundtrip: decode the multibase back to the same public key bytes.
        let decoded = decode_public_key_multibase(&kp.public_key_multibase).unwrap();
        assert_eq!(decoded.to_bytes(), kp.public_key.to_bytes());
    }

    #[test]
    fn test_did_web_construction() {
        // Root-domain form
        let did = build_did_web("org.example", &[]);
        assert_eq!(did, "did:web:org.example");
        let url = did_web_document_url("org.example", &[]);
        assert_eq!(url, "https://org.example/.well-known/did.json");

        // Sub-path form
        let did = build_did_web("credbrew.example", &["tenants", "techconf"]);
        assert_eq!(did, "did:web:credbrew.example:tenants:techconf");
        let url = did_web_document_url("credbrew.example", &["tenants", "techconf"]);
        assert_eq!(url, "https://credbrew.example/tenants/techconf/did.json");
    }

    #[test]
    fn test_did_document_shape() {
        let kp = generate_keypair();
        let did = "did:web:credbrew.example:tenants:techconf";
        let doc = build_did_document(did, "key-1", &kp.public_key_multibase);

        assert_eq!(doc["id"], did);
        assert_eq!(doc["verificationMethod"][0]["id"], format!("{did}#key-1"));
        assert_eq!(
            doc["verificationMethod"][0]["type"],
            "Ed25519VerificationKey2020"
        );
        assert_eq!(
            doc["verificationMethod"][0]["publicKeyMultibase"],
            kp.public_key_multibase
        );
        assert_eq!(doc["authentication"][0], format!("{did}#key-1"));
        assert_eq!(doc["assertionMethod"][0], format!("{did}#key-1"));
    }

    #[test]
    fn test_sign_and_verify_roundtrip() {
        let kp = generate_keypair();
        let signing_key = signing_key_from_seed(&kp.seed);
        let did = "did:web:credbrew.example:tenants:test";
        let vm = format!("{did}#key-1");

        let doc = json!({
            "@context": ["https://www.w3.org/2018/credentials/v1"],
            "id": "https://credbrew.example/credentials/test-1",
            "type": ["VerifiableCredential"],
            "issuer": did,
            "issuanceDate": "2026-11-08T10:00:00Z",
            "credentialSubject": {
                "id": "mailto:alice@example.com",
                "name": "Alice Chen"
            }
        });

        let (proof, signed_doc) = sign_document(&doc, &vm, &signing_key).unwrap();
        assert!(proof.proof_value.is_some());

        // The signed doc now has the proof block with proofValue.
        assert!(signed_doc["proof"]["proofValue"].is_string());

        // Verify with the correct public key.
        let ok = verify_document(&signed_doc, &kp.public_key).unwrap();
        assert!(ok, "verification with correct key should succeed");

        // Verify with a wrong key fails.
        let other = generate_keypair();
        let ok_wrong = verify_document(&signed_doc, &other.public_key).unwrap();
        assert!(!ok_wrong, "verification with wrong key should fail");
    }

    #[test]
    fn test_canonicalize_is_deterministic() {
        // Two objects with the same content but different key order must
        // canonicalize to the same bytes.
        let a = json!({"b": 1, "a": 2, "c": [3, 2, 1]});
        let b = json!({"c": [3, 2, 1], "a": 2, "b": 1});
        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn test_tampered_document_fails_verification() {
        let kp = generate_keypair();
        let signing_key = signing_key_from_seed(&kp.seed);
        let did = "did:web:credbrew.example:tenants:test";
        let vm = format!("{did}#key-1");

        let doc = json!({
            "@context": ["https://www.w3.org/2018/credentials/v1"],
            "id": "https://credbrew.example/credentials/test-1",
            "type": ["VerifiableCredential"],
            "issuer": did,
            "issuanceDate": "2026-11-08T10:00:00Z",
            "credentialSubject": { "id": "mailto:alice@example.com" }
        });

        let (_proof, mut signed) = sign_document(&doc, &vm, &signing_key).unwrap();

        // Tamper with the subject name.
        signed["credentialSubject"]["name"] = json!("Mallory");

        let ok = verify_document(&signed, &kp.public_key).unwrap();
        assert!(!ok, "tampered document must fail verification");
    }
}
