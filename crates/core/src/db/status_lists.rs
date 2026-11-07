//! Status list operations for credential revocation (BitstringStatusList 2021).
//!
//! Phase 3 implements allocation (reserve a bit index when issuing) and
//! reading (check the bit during verification). Phase 4 will add the
//! signed StatusListCredential serving and the actual bit-flip on revoke.
//!
//! See: <https://www.w3.org/TR/vc-bitstring-status-list/>
//! See: CredBrew v1.0 Design RFC §7

use crate::error::CoreError;
use crate::models::StatusList;
use sqlx::PgPool;
use uuid::Uuid;

const STATUS_COLUMNS: &str = "id, tenant_id, index, bitstring, vc_id, updated_at";

/// Get or create the latest (highest-index) status list for a tenant.
/// Lists hold 131,072 bits (16,384 bytes) each. When a list fills, a new
/// one with index+1 is created. Returns the list and whether it was newly
/// created.
pub async fn get_or_create_latest_list(
    pool: &PgPool,
    tenant_id: Uuid,
    vc_id_template: impl Fn(i32) -> String,
) -> Result<(StatusList, bool), CoreError> {
    // Try to load the highest-index list for this tenant.
    let existing = sqlx::query_as::<_, StatusList>(&format!(
        r#"
        SELECT {STATUS_COLUMNS}
        FROM status_lists
        WHERE tenant_id = $1
        ORDER BY index DESC
        LIMIT 1
        "#
    ))
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;

    if let Some(list) = existing {
        // If the list is full, create a new one with index+1.
        if list_is_full(&list) {
            let next_index = list.index + 1;
            let new_list =
                create_list(pool, tenant_id, next_index, vc_id_template(next_index)).await?;
            Ok((new_list, true))
        } else {
            Ok((list, false))
        }
    } else {
        // No list exists yet — create the first one.
        let list = create_list(pool, tenant_id, 1, vc_id_template(1)).await?;
        Ok((list, true))
    }
}

async fn create_list(
    pool: &PgPool,
    tenant_id: Uuid,
    index: i32,
    vc_id: String,
) -> Result<StatusList, CoreError> {
    // 16384 bytes of zeros = 131,072 bits, all unset (not revoked).
    let bitstring = vec![0u8; 16384];
    let list = sqlx::query_as::<_, StatusList>(&format!(
        r#"
        INSERT INTO status_lists (tenant_id, index, bitstring, vc_id)
        VALUES ($1, $2, $3, $4)
        RETURNING {STATUS_COLUMNS}
        "#
    ))
    .bind(tenant_id)
    .bind(index)
    .bind(&bitstring)
    .bind(&vc_id)
    .fetch_one(pool)
    .await?;
    Ok(list)
}

/// True if every bit in the list is allocated. We track allocation by
/// counting non-zero bytes; since we only ever set bits (never clear),
/// a full list has every bit set. For revocation purposes, "full" means
/// all 131,072 bits are 1 (every slot revoked) — which in practice never
/// happens before a tenant issues 131k credentials. We use a simpler
/// heuristic here: a list is full when its bitstring is all 0xFF.
/// Phase 4 will replace this with an allocation counter column.
fn list_is_full(list: &StatusList) -> bool {
    list.bitstring.iter().all(|&b| b == 0xFF)
}

/// Read the bit at `index` in the status list. Returns true if set
/// (revoked), false if unset (active).
pub fn get_bit(list: &StatusList, index: u32) -> bool {
    let byte_idx = (index / 8) as usize;
    let bit_idx = 7 - (index % 8) as u8;
    if byte_idx >= list.bitstring.len() {
        return false;
    }
    (list.bitstring[byte_idx] >> bit_idx) & 1 == 1
}

/// Set the bit at `index` in the status list (mark as revoked).
/// Persists the updated bitstring to the database.
pub async fn set_bit(pool: &PgPool, list_id: Uuid, index: u32) -> Result<(), CoreError> {
    // Fetch, modify, update. The bitstring is 16KB; we read it, flip the
    // bit, write it back. Phase 4 will wrap this in a transaction with
    // the StatusListCredential re-signing.
    let list = sqlx::query_as::<_, StatusList>(&format!(
        r#"
        SELECT {STATUS_COLUMNS}
        FROM status_lists
        WHERE id = $1
        "#,
    ))
    .bind(list_id)
    .fetch_one(pool)
    .await?;

    let mut bitstring = list.bitstring.clone();
    let byte_idx = (index / 8) as usize;
    let bit_idx = 7 - (index % 8) as u8;
    if byte_idx < bitstring.len() {
        bitstring[byte_idx] |= 1 << bit_idx;
    }

    sqlx::query(
        r#"
        UPDATE status_lists
        SET bitstring = $2, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(list_id)
    .bind(&bitstring)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get a status list by id (used by the verify flow to check a credential's
/// revocation status).
pub async fn get_by_id(pool: &PgPool, id: Uuid) -> Result<Option<StatusList>, CoreError> {
    let list = sqlx::query_as::<_, StatusList>(&format!(
        r#"
        SELECT {STATUS_COLUMNS}
        FROM status_lists
        WHERE id = $1
        "#
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_bit_unset() {
        let list = StatusList {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            index: 1,
            bitstring: vec![0u8; 16],
            vc_id: "test".to_string(),
            updated_at: chrono::Utc::now(),
        };
        // All bits in an all-zero list are unset.
        for i in 0..64 {
            assert!(!get_bit(&list, i), "bit {} should be unset", i);
        }
    }

    #[test]
    fn test_get_bit_set_byte0() {
        // 0b10000000 = bit 0 set (we use big-endian bit order within a byte)
        let mut bytes = vec![0u8; 16];
        bytes[0] = 0b1000_0000;
        let list = StatusList {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            index: 1,
            bitstring: bytes,
            vc_id: "test".to_string(),
            updated_at: chrono::Utc::now(),
        };
        assert!(get_bit(&list, 0));
        assert!(!get_bit(&list, 1));
        assert!(!get_bit(&list, 7));
    }

    #[test]
    fn test_get_bit_byte_boundary() {
        // Index 8 is the high bit of byte 1
        let mut bytes = vec![0u8; 16];
        bytes[1] = 0b1000_0000;
        let list = StatusList {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            index: 1,
            bitstring: bytes,
            vc_id: "test".to_string(),
            updated_at: chrono::Utc::now(),
        };
        assert!(!get_bit(&list, 7));
        assert!(get_bit(&list, 8));
        assert!(!get_bit(&list, 9));
    }

    #[test]
    fn test_get_bit_out_of_range_returns_false() {
        let list = StatusList {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            index: 1,
            bitstring: vec![0xFFu8; 4], // only 32 bits
            vc_id: "test".to_string(),
            updated_at: chrono::Utc::now(),
        };
        // In-range bits are all set
        assert!(get_bit(&list, 0));
        assert!(get_bit(&list, 31));
        // Out-of-range returns false (defensive)
        assert!(!get_bit(&list, 32));
        assert!(!get_bit(&list, 1000));
    }

    #[test]
    fn test_list_full_heuristic() {
        let make_list = |bytes: Vec<u8>| StatusList {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            index: 1,
            bitstring: bytes,
            vc_id: "test".to_string(),
            updated_at: chrono::Utc::now(),
        };
        assert!(!list_is_full(&make_list(vec![0u8; 16])));
        assert!(!list_is_full(&make_list(vec![0xFFu8, 0x00])));
        assert!(list_is_full(&make_list(vec![0xFFu8; 16])));
    }
}
