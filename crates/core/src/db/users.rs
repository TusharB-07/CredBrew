use crate::error::CoreError;
use crate::models::User;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_user(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
    name: &str,
) -> Result<User, CoreError> {
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (email, password_hash, name)
        VALUES ($1, $2, $3)
        RETURNING id, email, password_hash, name, created_at
        "#,
        email,
        password_hash,
        name
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn get_user_by_email(pool: &PgPool, email: &str) -> Result<Option<User>, CoreError> {
    let user = sqlx::query_as!(
        User,
        r#"
        SELECT id, email, password_hash, name, created_at
        FROM users
        WHERE email = $1
        "#,
        email
    )
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn get_user_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, CoreError> {
    let user = sqlx::query_as!(
        User,
        r#"
        SELECT id, email, password_hash, name, created_at
        FROM users
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

/// List all users. Used by the v0.1 → v1.0 backfill binary.
// Runtime-checked query (not query_as!) because the .sqlx offline cache
// doesn't have this query and we can't run a live DB during build.
pub async fn get_all_users(pool: &PgPool) -> Result<Vec<User>, CoreError> {
    let users = sqlx::query_as::<_, User>(
        r#"
        SELECT id, email, password_hash, name, created_at
        FROM users
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}
