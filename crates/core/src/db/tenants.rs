use crate::error::CoreError;
use crate::models::{ActiveTenant, Tenant, TenantMembership, TenantRole};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Tenant CRUD
//
// Uses runtime-checked `sqlx::query_as` (not the `query_as!` macro) so we can
// add new queries without regenerating the .sqlx offline cache. The trade-off
// is losing compile-time SQL validation on these queries; the integration
// tests in `tests/tenants.rs` cover the SQL surface.
// ─────────────────────────────────────────────────────────────────────────────

const TENANT_COLUMNS: &str = "id, slug, name, domain, plan, anchoring_enabled, created_at";

fn slugify(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' => c,
            _ => '-',
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "tenant".to_string()
    } else {
        slug
    }
}

/// Generate a unique slug by appending `-2`, `-3`, ... until free.
async fn unique_slug(pool: &PgPool, base: &str) -> Result<String, CoreError> {
    let mut slug = base.to_string();
    let mut n = 2u32;
    loop {
        let taken: bool = sqlx::query("SELECT EXISTS(SELECT 1 FROM tenants WHERE slug = $1)")
            .bind(&slug)
            .fetch_one(pool)
            .await?
            .get(0);
        if !taken {
            return Ok(slug);
        }
        slug = format!("{base}-{n}");
        n += 1;
    }
}

pub async fn create_tenant(
    pool: &PgPool,
    name: &str,
    slug: Option<&str>,
    plan: &str,
) -> Result<Tenant, CoreError> {
    let base_slug = slug.map(|s| s.to_string()).unwrap_or_else(|| slugify(name));
    let slug = unique_slug(pool, &base_slug).await?;

    let tenant = sqlx::query_as::<_, Tenant>(&format!(
        "INSERT INTO tenants (slug, name, plan) VALUES ($1, $2, $3) RETURNING {TENANT_COLUMNS}"
    ))
    .bind(&slug)
    .bind(name)
    .bind(plan)
    .fetch_one(pool)
    .await?;

    Ok(tenant)
}

pub async fn get_tenant(pool: &PgPool, id: Uuid) -> Result<Option<Tenant>, CoreError> {
    let tenant = sqlx::query_as::<_, Tenant>(&format!(
        "SELECT {TENANT_COLUMNS} FROM tenants WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(tenant)
}

pub async fn get_tenant_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Tenant>, CoreError> {
    let tenant = sqlx::query_as::<_, Tenant>(&format!(
        "SELECT {TENANT_COLUMNS} FROM tenants WHERE slug = $1"
    ))
    .bind(slug)
    .fetch_optional(pool)
    .await?;
    Ok(tenant)
}

pub async fn list_tenants_for_user(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<(Tenant, TenantRole)>, CoreError> {
    let rows = sqlx::query(&format!(
        r#"
        SELECT t.{TENANT_COLS}, tm.role as "role: TenantRole", tm.created_at as member_since
        FROM tenants t
        JOIN tenant_memberships tm ON tm.tenant_id = t.id
        WHERE tm.user_id = $1
        ORDER BY tm.created_at
        "#,
        TENANT_COLS = TENANT_COLUMNS
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    // Manual mapping — sqlx::FromRow on a join with a role enum needs help.
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let tenant = Tenant {
            id: row.get("id"),
            slug: row.get("slug"),
            name: row.get("name"),
            domain: row.get("domain"),
            plan: row.get("plan"),
            anchoring_enabled: row.get("anchoring_enabled"),
            created_at: row.get("created_at"),
        };
        let role: TenantRole = row
            .try_get("role")
            .map_err(CoreError::Database)?;
        out.push((tenant, role));
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Memberships
// ─────────────────────────────────────────────────────────────────────────────

pub async fn add_membership(
    pool: &PgPool,
    user_id: Uuid,
    tenant_id: Uuid,
    role: TenantRole,
) -> Result<TenantMembership, CoreError> {
    let membership = sqlx::query_as::<_, TenantMembership>(
        r#"
        INSERT INTO tenant_memberships (user_id, tenant_id, role)
        VALUES ($1, $2, $3)
        RETURNING user_id, tenant_id, role as "role: TenantRole", created_at
        "#,
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(role as TenantRole)
    .fetch_one(pool)
    .await?;
    Ok(membership)
}

pub async fn get_membership(
    pool: &PgPool,
    user_id: Uuid,
    tenant_id: Uuid,
) -> Result<Option<TenantMembership>, CoreError> {
    let membership = sqlx::query_as::<_, TenantMembership>(
        r#"
        SELECT user_id, tenant_id, role as "role: TenantRole", created_at
        FROM tenant_memberships
        WHERE user_id = $1 AND tenant_id = $2
        "#,
    )
    .bind(user_id)
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(membership)
}

/// Resolve the active tenant for a request. If `header_tenant_id` is provided,
/// verify the user has a membership in that tenant; otherwise default to the
/// user's first (oldest) tenant. Returns None if the user has no tenants.
pub async fn resolve_active_tenant(
    pool: &PgPool,
    user_id: Uuid,
    header_tenant_id: Option<Uuid>,
) -> Result<Option<ActiveTenant>, CoreError> {
    if let Some(tid) = header_tenant_id {
        if let Some(m) = get_membership(pool, user_id, tid).await? {
            if let Some(t) = get_tenant(pool, tid).await? {
                return Ok(Some(ActiveTenant {
                    tenant: t,
                    role: m.role,
                }));
            }
        }
        // Header specified a tenant the user doesn't belong to — fall through
        // to default rather than 403, so a stale client-side tenant cookie
        // doesn't lock the user out.
    }

    // Default: first (oldest) membership.
    let rows = sqlx::query(&format!(
        r#"
        SELECT t.{TENANT_COLS}, tm.role as "role: TenantRole"
        FROM tenants t
        JOIN tenant_memberships tm ON tm.tenant_id = t.id
        WHERE tm.user_id = $1
        ORDER BY tm.created_at
        LIMIT 1
        "#,
        TENANT_COLS = TENANT_COLUMNS
    ))
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = rows else { return Ok(None) };
    let tenant = Tenant {
        id: row.get("id"),
        slug: row.get("slug"),
        name: row.get("name"),
        domain: row.get("domain"),
        plan: row.get("plan"),
        anchoring_enabled: row.get("anchoring_enabled"),
        created_at: row.get("created_at"),
    };
    let role: TenantRole = row
        .try_get("role")
        .map_err(CoreError::Database)?;
    Ok(Some(ActiveTenant { tenant, role }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Convenience: create a tenant and add the creator as owner in one call.
// Used by the backfill binary and the (future) tenant signup route.
// ─────────────────────────────────────────────────────────────────────────────

pub async fn create_tenant_with_owner(
    pool: &PgPool,
    name: &str,
    slug: Option<&str>,
    plan: &str,
    owner_user_id: Uuid,
) -> Result<(Tenant, TenantMembership), CoreError> {
    let tenant = create_tenant(pool, name, slug, plan).await?;
    let membership = add_membership(pool, owner_user_id, tenant.id, TenantRole::Owner).await?;
    Ok((tenant, membership))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_slugify_basic() {
        assert_eq!(slugify("TechConf 2025"), "techconf-2025");
        // trim_matches('-') strips leading/trailing dashes
        assert_eq!(slugify("  Hello, World!  "), "hello--world");
    }

    #[test]
    fn test_slugify_empty_falls_back() {
        assert_eq!(slugify(""), "tenant");
        assert_eq!(slugify("!!!"), "tenant");
    }

    #[test]
    fn test_slugify_unicode_drops_to_dashes() {
        // Non-ASCII letters become dashes; the trim happens at the DB level
        // via unique_slug, which is hard to unit-test without a pool.
        let s = slugify("Café Crème");
        assert!(s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn test_tenant_role_parsing() {
        assert_eq!(TenantRole::from_str("owner").unwrap(), TenantRole::Owner);
        assert_eq!(TenantRole::from_str("ADMIN").unwrap(), TenantRole::Admin);
        assert_eq!(TenantRole::from_str("member").unwrap(), TenantRole::Member);
        assert!(TenantRole::from_str("superuser").is_err());
    }

    #[test]
    fn test_tenant_role_can_admin() {
        assert!(TenantRole::Owner.can_admin());
        assert!(TenantRole::Admin.can_admin());
        assert!(!TenantRole::Member.can_admin());
    }
}
