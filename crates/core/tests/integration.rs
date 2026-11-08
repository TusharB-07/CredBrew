//! Integration tests for CredBrew v1.0.
//!
//! These tests require a running PostgreSQL instance (with migrations applied)
//! and exercise the DB layer end-to-end. They do NOT test the HTTP routes
//! (that would require starting the full Axum server); they test the
//! `credbrew_core::db` modules directly.
//!
//! ## Running
//!
//! ```bash
//! # 1. Start infra
//! docker compose up -d
//!
//! # 2. Run migrations
//! sqlx migrate run --database-url postgres://credbrew:credbrew@localhost:5432/credbrew
//!
//! # 3. Run the integration tests
//! DATABASE_URL=postgres://credbrew:credbrew@localhost:5432/credbrew \
//! CREDBREW_MASTER_KEY=test-master-key-for-integration-tests \
//! cargo test --test integration -- --test-threads=1
//! ```
//!
//! The tests use `--test-threads=1` because they share a single database and
//! would race on tenant IDs / short codes if run in parallel. A production
//! test suite would use a separate test database per thread.

use chrono::NaiveDate;
use credbrew_core::{
    crypto::{KeyStore, LocalFileKeyStore},
    db,
    did::{self, generate_keypair, sign_document, signing_key_from_seed, verify_document},
    models::*,
};
use sqlx::PgPool;

/// Helper: create a test user + tenant.
async fn setup_test_tenant(pool: &PgPool) -> (User, Tenant) {
    let email = format!("test-{}@credbrew.dev", uuid::Uuid::new_v4());
    let user = db::users::create_user(pool, &email, "dummyhash", "Test User")
        .await
        .expect("create_user failed");
    let (tenant, _m) =
        db::tenants::create_tenant_with_owner(pool, "Test Workspace", None, "free", user.id)
            .await
            .expect("create_tenant_with_owner failed");
    (user, tenant)
}

/// Helper: create a test event under a tenant.
async fn create_test_event(pool: &PgPool, owner_id: uuid::Uuid, tenant_id: uuid::Uuid) -> Event {
    db::events::create_event(
        pool,
        owner_id,
        tenant_id,
        "TestConf 2026",
        Some(NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()),
        "Test Org",
        "#4F46E5",
    )
    .await
    .expect("create_event failed")
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: multi-tenant isolation
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_tenant_isolation(pool: PgPool) {
    // Create two users, each with their own tenant.
    let (user_a, tenant_a) = setup_test_tenant(&pool).await;
    let (user_b, tenant_b) = setup_test_tenant(&pool).await;

    // User A creates an event in tenant A.
    let event_a = create_test_event(&pool, user_a.id, tenant_a.id).await;
    // User B creates an event in tenant B.
    let event_b = create_test_event(&pool, user_b.id, tenant_b.id).await;

    // User A lists events for tenant A → should see only event_a.
    let events_a = db::events::list_events_for_tenant(&pool, tenant_a.id)
        .await
        .expect("list_events_for_tenant A failed");
    assert_eq!(events_a.len(), 1);
    assert_eq!(events_a[0].id, event_a.id);

    // User B lists events for tenant B → should see only event_b.
    let events_b = db::events::list_events_for_tenant(&pool, tenant_b.id)
        .await
        .expect("list_events_for_tenant B failed");
    assert_eq!(events_b.len(), 1);
    assert_eq!(events_b[0].id, event_b.id);

    // Cross-tenant access: tenant A tries to fetch event_b → should get None.
    let cross = db::events::get_event_for_tenant(&pool, event_b.id, tenant_a.id)
        .await
        .expect("get_event_for_tenant failed");
    assert!(cross.is_none(), "tenant A should not see tenant B's event");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: DID minting + VC signing + verification roundtrip (DB-backed)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_did_mint_and_sign_roundtrip(pool: PgPool) {
    let (_user, tenant) = setup_test_tenant(&pool).await;

    // Mint a DID for the tenant.
    let kp = generate_keypair();
    let did_string = did::build_did_web("credbrew.example", &["tenants", &tenant.slug]);
    let did_doc = did::build_did_document(&did_string, "key-1", &kp.public_key_multibase);

    let issuer_did =
        db::issuer_dids::create_issuer_did(&pool, tenant.id, &did_string, "web", &did_doc)
            .await
            .expect("create_issuer_did failed");

    // Seal the key + persist.
    let keystore = LocalFileKeyStore::from_passphrase("test-master-key-for-integration-tests");
    let sealed = keystore.seal(&kp.seed).expect("seal failed");
    let _signing_key = db::issuer_dids::create_signing_key(
        &pool,
        issuer_did.id,
        "key-1",
        &kp.public_key_multibase,
        &sealed,
    )
    .await
    .expect("create_signing_key failed");

    // Load the decrypted key + sign a sample VC.
    let dk = db::issuer_dids::get_decrypted_active_signing_key(&pool, tenant.id, &keystore)
        .await
        .expect("get_decrypted_active_signing_key failed");

    assert_eq!(dk.did, did_string);
    assert_eq!(dk.key_id, "key-1");

    let signing_key = signing_key_from_seed(&dk.seed);
    let vm = format!("{}#{}", dk.did, dk.key_id);

    // Build + sign a sample VC.
    let sample_vc = serde_json::json!({
        "@context": ["https://www.w3.org/2018/credentials/v1"],
        "id": "https://credbrew.example/api/public/vc/test-1",
        "type": ["VerifiableCredential"],
        "issuer": did_string,
        "issuanceDate": "2026-11-08T10:00:00Z",
        "credentialSubject": { "id": "mailto:alice@credbrew.dev" }
    });

    let (proof, signed_vc) =
        sign_document(&sample_vc, &vm, &signing_key).expect("sign_document failed");
    assert!(proof.proof_value.is_some());

    // Verify with the correct public key.
    let ok = verify_document(&signed_vc, &kp.public_key).expect("verify_document failed");
    assert!(ok, "signed VC must verify with the issuer's public key");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: magic-link create + consume
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_magic_link_lifecycle(pool: PgPool) {
    let email = format!("recipient-{}@credbrew.dev", uuid::Uuid::new_v4());

    // Create a magic link.
    let token = db::magic_links::create_magic_link(&pool, &email)
        .await
        .expect("create_magic_link failed");
    assert!(!token.is_empty());

    // Consume it → should return the email.
    let consumed = db::magic_links::consume_magic_link(&pool, &token)
        .await
        .expect("consume_magic_link failed");
    assert_eq!(consumed.as_deref(), Some(email.as_str()));

    // Try to consume it again → should fail (single-use).
    let replay = db::magic_links::consume_magic_link(&pool, &token)
        .await
        .expect("consume_magic_link (replay) failed");
    assert!(replay.is_none(), "magic link should be single-use");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: status list bit flip on revocation
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_status_list_revocation(pool: PgPool) {
    let (_user, tenant) = setup_test_tenant(&pool).await;

    // Get or create a status list for the tenant.
    let (list, _created) = db::status_lists::get_or_create_latest_list(&pool, tenant.id, |_| {
        format!("https://credbrew.example/tenants/{}/status/1", tenant.slug)
    })
    .await
    .expect("get_or_create_latest_list failed");

    // Verify all bits start unset.
    for i in 0..16 {
        assert!(
            !db::status_lists::get_bit(&list, i),
            "bit {} should be unset",
            i
        );
    }

    // Set bit 5.
    db::status_lists::set_bit(&pool, list.id, 5)
        .await
        .expect("set_bit failed");

    // Reload and verify bit 5 is set, others are not.
    let reloaded = db::status_lists::get_by_id(&pool, list.id)
        .await
        .expect("get_by_id failed")
        .expect("list should exist");
    assert!(
        db::status_lists::get_bit(&reloaded, 5),
        "bit 5 should be set"
    );
    assert!(
        !db::status_lists::get_bit(&reloaded, 4),
        "bit 4 should still be unset"
    );
    assert!(
        !db::status_lists::get_bit(&reloaded, 6),
        "bit 6 should still be unset"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: anchor job enqueue + dequeue batching
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_anchor_job_queue(pool: PgPool) {
    // Enqueue 3 jobs for the same chain.
    let cred_id_1 = uuid::Uuid::new_v4();
    let cred_id_2 = uuid::Uuid::new_v4();
    let cred_id_3 = uuid::Uuid::new_v4();

    db::anchor_jobs::enqueue(&pool, cred_id_1, "polygon", "hash1")
        .await
        .expect("enqueue 1 failed");
    db::anchor_jobs::enqueue(&pool, cred_id_2, "polygon", "hash2")
        .await
        .expect("enqueue 2 failed");
    db::anchor_jobs::enqueue(&pool, cred_id_3, "polygon", "hash3")
        .await
        .expect("enqueue 3 failed");

    // Dequeue a batch of 2.
    let (jobs, batch_id) = db::anchor_jobs::dequeue_batch(&pool, "polygon", 2)
        .await
        .expect("dequeue_batch failed");
    assert_eq!(jobs.len(), 2, "should have dequeued 2 jobs");
    assert!(batch_id > 0, "batch_id should be positive");

    // All dequeued jobs should be 'submitted'.
    for job in &jobs {
        assert_eq!(job.status, "submitted");
        assert_eq!(job.batch_id, Some(batch_id));
    }

    // Dequeue the remaining 1.
    let (jobs2, batch_id2) = db::anchor_jobs::dequeue_batch(&pool, "polygon", 2)
        .await
        .expect("dequeue_batch 2 failed");
    assert_eq!(jobs2.len(), 1, "should have dequeued 1 remaining job");
    assert_ne!(
        batch_id, batch_id2,
        "different batches should have different IDs"
    );

    // Confirm the first batch.
    let n = db::anchor_jobs::confirm_batch(&pool, batch_id, "0xabc123", 12345)
        .await
        .expect("confirm_batch failed");
    assert_eq!(n, 2, "should have confirmed 2 jobs");
}
