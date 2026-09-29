use axum::{
    extract::{State, Path, Multipart},
    Json,
    response::Redirect,
};
use uuid::Uuid;
use serde::Serialize;
use crate::state::AppState;
use crate::routes::auth::AuthUser;
use credbrew_core::{
    db::{templates, participant_types, events},
    models::{Template, TemplateKind, NameZone, QrZone},
    render::{sanitize_svg, render_png, render_cert_png},
    error::CoreError,
};

#[derive(Serialize)]
pub struct TemplateResponse {
    #[serde(flatten)]
    pub template: Template,
}

#[axum::debug_handler]
pub async fn create_template(
    State(state): State<std::sync::Arc<AppState>>,
    user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<TemplateResponse>, CoreError> {
    let mut participant_type_id = None;
    let mut kind = None;
    let mut name_zone = None;
    let mut qr_zone = None;
    let mut svg_data = None;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("");
        match name {
            "participant_type_id" => {
                let text = field.text().await?;
                participant_type_id = Some(text.parse()?);
            }
            "kind" => {
                let text = field.text().await?;
                kind = Some(text.parse::<TemplateKind>()?);
            }
            "name_zone" => {
                let text = field.text().await?;
                name_zone = Some(serde_json::from_str::<NameZone>(&text)?);
            }
            "qr_zone" => {
                let text = field.text().await?;
                qr_zone = Some(serde_json::from_str::<QrZone>(&text)?);
            }
            "file" => {
                let data = field.bytes().await?;
                if data.len() > 2 * 1024 * 1024 {
                    return Err(CoreError::TemplateValidation("SVG file too large (max 2MB)".into()));
                }
                svg_data = Some(data);
            }
            _ => {}
        }
    }

    let participant_type_id = participant_type_id.ok_or_else(|| CoreError::TemplateValidation("participant_type_id required".into()))?;
    let kind = kind.ok_or_else(|| CoreError::TemplateValidation("kind required".into()))?;
    let name_zone = name_zone.ok_or_else(|| CoreError::TemplateValidation("name_zone required".into()))?;
    let qr_zone = qr_zone.ok_or_else(|| CoreError::TemplateValidation("qr_zone required".into()))?;
    let svg_data = svg_data.ok_or_else(|| CoreError::TemplateValidation("SVG file required".into()))?;

    let pt = participant_types::get_participant_type(&state.pool, participant_type_id).await?
        .ok_or_else(|| CoreError::NotFound("participant type not found".into()))?;

    let event = events::get_event(&state.pool, pt.event_id).await?
        .ok_or_else(|| CoreError::NotFound("event not found".into()))?;

    if event.owner_id != user.id {
        return Err(CoreError::Forbidden("not event owner".into()));
    }

    let svg_str = std::str::from_utf8(&svg_data)
        .map_err(|_| CoreError::TemplateValidation("SVG must be valid UTF-8".into()))?;

    let sanitized = sanitize_svg(svg_str)?;

    if !sanitized.contains("{{NAME}}") {
        return Err(CoreError::TemplateValidation("SVG must contain {{NAME}} placeholder".into()));
    }

    if !sanitized.contains("id=\"qr-zone\"") {
        return Err(CoreError::TemplateValidation("SVG must contain an element with id=\"qr-zone\"".into()));
    }

    let file_key = format!("templates/{}/{}.svg", participant_type_id, kind);
    state.storage.put(&file_key, sanitized.into_bytes(), "image/svg+xml").await?;

    let template = templates::create_template(
        &state.pool,
        participant_type_id,
        kind,
        &file_key,
        serde_json::to_value(name_zone)?,
        serde_json::to_value(qr_zone)?,
    ).await?;

    Ok(Json(TemplateResponse { template }))
}

#[axum::debug_handler]
pub async fn preview_template(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, CoreError> {
    let template = templates::get_template(&state.pool, id).await?
        .ok_or_else(|| CoreError::NotFound("template not found".into()))?;

    let svg_bytes = state.storage.get(&template.file_key).await?;
    let svg_str = std::str::from_utf8(&svg_bytes)?;

    let mut svg_with_name = credbrew_core::render::inject_name(
        svg_str,
        "Jane Doe",
        &serde_json::from_value(template.name_zone.clone())?,
    )?;

    let short_code = "PREVIEW123";
    svg_with_name = credbrew_core::render::inject_qr(
        &svg_with_name,
        short_code,
        &state.config.public_base_url,
        &serde_json::from_value(template.qr_zone.clone())?,
    )?;

    let (content_type, body) = match template.kind {
        TemplateKind::Badge => {
            let png = render_png(&svg_with_name)?;
            ("image/png", axum::body::Body::from(png))
        }
        TemplateKind::Certificate => {
            let png = render_cert_png(&svg_with_name)?;
            ("image/png", axum::body::Body::from(png))
        }
    };

    let mut response = axum::response::Response::new(body);
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        content_type.parse().unwrap(),
    );
    Ok(response)
}

#[axum::debug_handler]
pub async fn get_template_file(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Redirect, CoreError> {
    let template = templates::get_template(&state.pool, id).await?
        .ok_or_else(|| CoreError::NotFound("template not found".into()))?;

    Ok(Redirect::to(&format!("/api/files/templates/{}", template.file_key)))
}