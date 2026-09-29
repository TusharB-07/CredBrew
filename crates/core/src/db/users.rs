use sqlx::PgPool;
use uuid::Uuid;
use crate::models::User;
use crate::error::CoreError;

pub async fn create_user(pool: &PgPool, email: &str, password_hash: &str, name: &str) -> Result<User, CoreError> {
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (email, password_hash, name)
        VALUES ($1, $2, $3)
        RETURNING id, email, password_hash, name, created_at
        "#,
        email, password_hash, name
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