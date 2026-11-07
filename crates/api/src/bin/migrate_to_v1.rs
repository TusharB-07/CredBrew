//! Backfill binary: migrate v0.1 single-user data into v1.0 multi-tenant shape.
//!
//! Run AFTER applying migration 0003_tenants.sql:
//!
//! ```bash
//! cargo run --bin migrate_to_v1
//! ```
//!
//! For every existing user, this creates a "Personal" tenant owned by that
//! user, then sets `tenant_id` on all their events (and cascades to
//! participant_types, participants, templates, credentials via the event_id
//! join — those tables also have a tenant_id column, which we backfill via
//! a single UPDATE ... FROM statement per table).

use credbrew_core::{db, error::CoreError};
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<(), CoreError> {
    credbrew_core::init_tracing();
    dotenv::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url).await?;

    tracing::info!("Running v0.1 → v1.0 tenant backfill…");

    // 1. Load all users.
    let users = db::users::get_all_users(&pool).await?;
    tracing::info!("Found {} users to migrate", users.len());

    let mut tenants_created = 0usize;
    let mut events_assigned = 0u64;

    for user in &users {
        // 2. Create a personal tenant for this user (idempotent: skip if the
        //    user already has any membership — supports re-running the binary).
        let existing = db::tenants::list_tenants_for_user(&pool, user.id).await?;
        let tenant = if let Some((t, _)) = existing.into_iter().next() {
            tracing::info!(
                "User {} already has tenant {}; skipping creation",
                user.id,
                t.slug
            );
            t
        } else {
            let tenant_name = if user.name.trim().is_empty() {
                "Personal"
            } else {
                &user.name
            };
            let (t, _m) = db::tenants::create_tenant_with_owner(
                &pool,
                &format!("{tenant_name}'s Workspace"),
                None,
                "free",
                user.id,
            )
            .await?;
            tracing::info!("Created tenant {} ({}) for user {}", t.slug, t.id, user.id);
            tenants_created += 1;
            t
        };

        // 3. Assign tenant_id to all this user's events.
        let n = db::events::assign_tenant_to_owner_events(&pool, user.id, tenant.id).await?;
        if n > 0 {
            tracing::info!(
                "Assigned {} events to tenant {} for user {}",
                n,
                tenant.id,
                user.id
            );
            events_assigned += n;
        }
    }

    // 4. Cascade tenant_id from events to child tables. These tables already
    //    have tenant_id columns (added in 0003_tenants.sql); we populate them
    //    from the parent event's tenant_id in one bulk UPDATE each.
    cascade_tenant_id(&pool, "participant_types", "event_id", "events", "id").await?;
    cascade_tenant_id(&pool, "participants", "event_id", "events", "id").await?;
    cascade_tenant_id(
        &pool,
        "templates",
        "participant_type_id",
        "participant_types",
        "id",
    )
    .await?;
    cascade_tenant_id(&pool, "credentials", "participant_id", "participants", "id").await?;

    tracing::info!(
        "Backfill complete: {} tenants created, {} events assigned, child tables cascaded",
        tenants_created,
        events_assigned
    );
    tracing::info!("Next step: run migration 0004 to set tenant_id NOT NULL, then drop the legacy owner_id-based access checks.");

    Ok(())
}

/// Set `tenant_id` on all rows of `child_table` from the parent table's
/// `tenant_id`, joining on `child_table.{child_fk} = parent.{parent_pk}`.
/// Only updates rows where `child_table.tenant_id IS NULL`.
async fn cascade_tenant_id(
    pool: &PgPool,
    child_table: &str,
    child_fk: &str,
    parent_table: &str,
    parent_pk: &str,
) -> Result<u64, CoreError> {
    // Table/column names are static strings in this binary (not user input),
    // so interpolation is safe here. Parameters wouldn't work for identifiers.
    let sql = format!(
        r#"
        UPDATE {child} AS c
        SET tenant_id = p.tenant_id
        FROM {parent} AS p
        WHERE c.{child_fk} = p.{parent_pk}
          AND c.tenant_id IS NULL
          AND p.tenant_id IS NOT NULL
        "#,
        child = child_table,
        parent = parent_table,
        child_fk = child_fk,
        parent_pk = parent_pk,
    );
    let result = sqlx::query(&sql).execute(pool).await?;
    tracing::info!(
        "Cascaded tenant_id to {} rows in {}",
        result.rows_affected(),
        child_table
    );
    Ok(result.rows_affected())
}
