use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHasher};
use credbrew_core::{
    db::{events, participant_types, participants, templates, tenants as tenant_db, users},
    error::CoreError,
    models::{NameZone, QrZone, TemplateKind},
    short_code::generate_unique_short_code,
    storage::{Config as StorageConfig, StorageClient},
};
use sqlx::PgPool;
use std::env;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), CoreError> {
    dotenv::dotenv().ok();
    credbrew_core::init_tracing();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url).await?;

    sqlx::migrate!("../../migrations").run(&pool).await?;

    let storage_config = StorageConfig::from_env()?;
    let storage = StorageClient::new(&storage_config)?;

    let email = "demo@credbrew.dev";
    let password = "opendemo123";

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string();

    let user = match users::get_user_by_email(&pool, email).await? {
        Some(u) => u,
        None => users::create_user(&pool, email, &password_hash, "Demo User").await?,
    };

    // v1.0: ensure the demo user has a personal tenant.
    let tenant = match tenant_db::list_tenants_for_user(&pool, user.id)
        .await?
        .into_iter()
        .next()
    {
        Some((t, _)) => t,
        None => {
            let (t, _m) =
                tenant_db::create_tenant_with_owner(&pool, "Demo Workspace", None, "free", user.id)
                    .await?;
            t
        }
    };

    let event = match events::list_events_for_tenant(&pool, tenant.id)
        .await?
        .first()
    {
        Some(e) => e.clone(),
        None => {
            events::create_event(
                &pool,
                user.id,
                tenant.id,
                "TechConf 2025",
                Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()),
                "TechConf Org",
                "#4F46E5",
            )
            .await?
        }
    };

    participant_types::create_default_types(&pool, event.id).await?;

    let types = participant_types::get_participant_types_by_event(&pool, event.id).await?;

    let sample_participants = [
        ("Alice Chen", "alice@example.com", "Speaker"),
        ("Bob Martinez", "bob@example.com", "Speaker"),
        ("Carol Kim", "carol@example.com", "Winner"),
        ("David Okafor", "david@example.com", "Winner"),
        ("Eva Andersson", "eva@example.com", "Attendee"),
        ("José García", "jose@example.com", "Attendee"),
        (
            "Hiroshi Tanaka \u{6771}\u{5B5D}",
            "hiroshi@example.com",
            "Attendee",
        ),
    ];

    for (name, email, type_name) in sample_participants {
        let pt = types.iter().find(|t| t.name == type_name).unwrap();
        let existing =
            participants::get_participant_by_event_and_email(&pool, event.id, email).await?;
        if existing.is_none() {
            let short_code = generate_unique_short_code(|code| {
                let pool = pool.clone();
                async move {
                    let existing = sqlx::query_scalar!(
                        "SELECT 1 FROM participants WHERE short_code = $1",
                        code
                    )
                    .fetch_optional(&pool)
                    .await?;
                    Ok(existing.is_some())
                }
            })
            .await?;
            participants::create_participant(&pool, event.id, pt.id, name, email, &short_code)
                .await?;
        }
    }

    let badge_svg = include_str!("../../../../crates/core/assets/templates/badge.svg");
    let cert_svg = include_str!("../../../../crates/core/assets/templates/certificate.svg");

    for pt in &types {
        let badge_key = format!("templates/{}/badge.svg", pt.id);
        storage
            .put(&badge_key, badge_svg.as_bytes().to_vec(), "image/svg+xml")
            .await?;

        let name_zone = NameZone {
            x_pct: 10.0,
            y_pct: 70.0,
            w_pct: 80.0,
            font_size_pct: 6.0,
            color: "#111111".into(),
            align: "center".into(),
        };
        let qr_zone = QrZone {
            x_pct: 85.0,
            y_pct: 5.0,
            size_pct: 10.0,
        };

        templates::create_template(
            &pool,
            pt.id,
            TemplateKind::Badge,
            &badge_key,
            serde_json::to_value(name_zone.clone())?,
            serde_json::to_value(qr_zone.clone())?,
        )
        .await?;

        let cert_key = format!("templates/{}/certificate.svg", pt.id);
        storage
            .put(&cert_key, cert_svg.as_bytes().to_vec(), "image/svg+xml")
            .await?;

        templates::create_template(
            &pool,
            pt.id,
            TemplateKind::Certificate,
            &cert_key,
            serde_json::to_value(name_zone)?,
            serde_json::to_value(qr_zone)?,
        )
        .await?;
    }

    let token = create_token(&user.id)?;
    println!("\n✅ Seed complete!");
    println!("Login: demo@credbrew.dev / opendemo123");
    println!("JWT Token: {}", token);
    println!("\nUse this token for API calls:");
    println!("Authorization: Bearer {}", token);

    Ok(())
}

fn create_token(user_id: &Uuid) -> Result<String, CoreError> {
    use chrono::{Duration, Utc};
    use credbrew_core::models::Claims;
    use jsonwebtoken::{encode, EncodingKey, Header};

    let jwt_secret = env::var("JWT_SECRET")
        .unwrap_or_else(|_| "change-me-64-chars-minimum-length-for-security".into());
    let exp = (Utc::now() + Duration::hours(24)).timestamp() as usize;
    let claims = Claims { sub: *user_id, exp };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )?;
    Ok(token)
}
