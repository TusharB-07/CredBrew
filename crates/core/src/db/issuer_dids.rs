use crate::crypto::KeyStore;
use crate::error::CoreError;
use crate::models::{DecryptedSigningKey, IssuerDid, SigningKey};
use sqlx::PgPool;
use uuid::Uuid;

const DID_COLUMNS: &str = "id, tenant_id, did, method, did_document, is_active, created_at";
const KEY_COLUMNS: &str
    = "id, issuer_did_id, key_id, public_key_multibase, encrypted_seed, status as \"status: SigningKeyStatus\", created_at, retired_at";

// ─────────────────────────────────────────────────────────────────────────────
// IssuerDID CRUD
// ─────────────────────────────────────────────────────────────────────────────

pub async fn create_issuer_did(
    pool: &PgPool,
    tenant_id: Uuid,
    did: &str,
    method: &str,
    did_document: &serde_json::Value,
) -> Result<IssuerDid, CoreError> {
    // Single active DID per tenant — deactivate any existing active first.
    sqlx::query(
        "UPDATE issuer_dids SET is_active = false WHERE tenant_id = $1 AND is_active = true",
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;

    let did_row = sqlx::query_as::<_, IssuerDid>(&format!(
        r#"
        INSERT INTO issuer_dids (tenant_id, did, method, did_document)
        VALUES ($1, $2, $3, $4)
        RETURNING {DID_COLUMNS}
        "#
    ))
    .bind(tenant_id)
    .bind(did)
    .bind(method)
    .bind(did_document)
    .fetch_one(pool)
    .await?;
    Ok(did_row)
}

pub async fn get_active_did_for_tenant(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Option<IssuerDid>, CoreError> {
    let did = sqlx::query_as::<_, IssuerDid>(&format!(
        r#"
        SELECT {DID_COLUMNS}
        FROM issuer_dids
        WHERE tenant_id = $1 AND is_active = true
        "#
    ))
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(did)
}

pub async fn get_did_by_id(pool: &PgPool, id: Uuid) -> Result<Option<IssuerDid>, CoreError> {
    let did = sqlx::query_as::<_, IssuerDid>(&format!(
        r#"
        SELECT {DID_COLUMNS}
        FROM issuer_dids
        WHERE id = $1
        "#
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(did)
}

pub async fn list_dids_for_tenant(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Vec<IssuerDid>, CoreError> {
    let dids = sqlx::query_as::<_, IssuerDid>(&format!(
        r#"
        SELECT {DID_COLUMNS}
        FROM issuer_dids
        WHERE tenant_id = $1
        ORDER BY created_at DESC
        "#
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    Ok(dids)
}

// ─────────────────────────────────────────────────────────────────────────────
// SigningKey CRUD
// ─────────────────────────────────────────────────────────────────────────────

pub async fn create_signing_key(
    pool: &PgPool,
    issuer_did_id: Uuid,
    key_id: &str,
    public_key_multibase: &str,
    encrypted_seed: &[u8],
) -> Result<SigningKey, CoreError> {
    // Single active key per DID — retire any existing active first.
    sqlx::query(
        r#"
        UPDATE signing_keys
        SET status = 'retired', retired_at = now()
        WHERE issuer_did_id = $1 AND status = 'active'
        "#,
    )
    .bind(issuer_did_id)
    .execute(pool)
    .await?;

    let key = sqlx::query_as::<_, SigningKey>(&format!(
        r#"
        INSERT INTO signing_keys (issuer_did_id, key_id, public_key_multibase, encrypted_seed)
        VALUES ($1, $2, $3, $4)
        RETURNING {KEY_COLUMNS}
        "#
    ))
    .bind(issuer_did_id)
    .bind(key_id)
    .bind(public_key_multibase)
    .bind(encrypted_seed)
    .fetch_one(pool)
    .await?;
    Ok(key)
}

pub async fn get_active_signing_key(
    pool: &PgPool,
    issuer_did_id: Uuid,
) -> Result<Option<SigningKey>, CoreError> {
    let key = sqlx::query_as::<_, SigningKey>(&format!(
        r#"
        SELECT {KEY_COLUMNS}
        FROM signing_keys
        WHERE issuer_did_id = $1 AND status = 'active'
        "#
    ))
    .bind(issuer_did_id)
    .fetch_optional(pool)
    .await?;
    Ok(key)
}

/// Load the active signing key for a tenant's active DID, decrypt the seed
/// with the provided KeyStore, and return a DecryptedSigningKey ready for use.
pub async fn get_decrypted_active_signing_key(
    pool: &PgPool,
    tenant_id: Uuid,
    keystore: &dyn KeyStore,
) -> Result<DecryptedSigningKey, CoreError> {
    let did = get_active_did_for_tenant(pool, tenant_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("tenant has no active issuer DID".into()))?;

    let key = get_active_signing_key(pool, did.id)
        .await?
        .ok_or_else(|| CoreError::NotFound("active issuer DID has no signing key".into()))?;

    let seed = keystore.unseal(&key.encrypted_seed)?;

    Ok(DecryptedSigningKey {
        signing_key_id: key.id,
        key_id: key.key_id,
        did: did.did,
        seed,
    })
}

pub async fn list_signing_keys_for_did(
    pool: &PgPool,
    issuer_did_id: Uuid,
) -> Result<Vec<SigningKey>, CoreError> {
    let keys = sqlx::query_as::<_, SigningKey>(&format!(
        r#"
        SELECT {KEY_COLUMNS}
        FROM signing_keys
        WHERE issuer_did_id = $1
        ORDER BY created_at DESC
        "#
    ))
    .bind(issuer_did_id)
    .fetch_all(pool)
    .await?;
    Ok(keys)
}

/// Mark a signing key as retired. The key remains resolvable in the DID
/// document so previously-issued credentials remain verifiable.
pub async fn retire_signing_key(pool: &PgPool, id: Uuid) -> Result<Option<SigningKey>, CoreError> {
    let key = sqlx::query_as::<_, SigningKey>(&format!(
        r#"
        UPDATE signing_keys
        SET status = 'retired', retired_at = now()
        WHERE id = $1 AND status = 'active'
        RETURNING {KEY_COLUMNS}
        "#
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(key)
}

/// Mark a signing key as revoked (key compromise). The key is removed from
/// the DID document; credentials signed by it will fail verification.
pub async fn revoke_signing_key(pool: &PgPool, id: Uuid) -> Result<Option<SigningKey>, CoreError> {
    let key = sqlx::query_as::<_, SigningKey>(&format!(
        r#"
        UPDATE signing_keys
        SET status = 'revoked', retired_at = now()
        WHERE id = $1
        RETURNING {KEY_COLUMNS}
        "#
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(key)
}
