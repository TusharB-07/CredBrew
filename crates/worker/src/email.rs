use lettre::{
    message::{header, Mailbox, MultiPart, SinglePart},
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use chrono::Datelike;
use credbrew_core::{
    models::{Participant, ParticipantType, Event, TemplateKind},
    storage::StorageClient,
    error::CoreError,
};
use percent_encoding::NON_ALPHANUMERIC;

#[derive(Clone)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_pass: String,
    pub smtp_from: String,
    pub public_base_url: String,
}

pub async fn send_credential_email(
    config: &EmailConfig,
    participant: &Participant,
    participant_type: &ParticipantType,
    event: &Event,
    kind: &TemplateKind,
    _file_key: &str,
    _storage: &StorageClient,
) -> Result<(), CoreError> {
    let verify_url = format!("{}/v/{}", config.public_base_url.trim_end_matches('/'), participant.short_code);
    let download_url = format!(
        "{}/api/public/verify/asset/{}",
        config.public_base_url.trim_end_matches('/'),
        participant.short_code
    );

    let linkedin_name = format!("{} — {}", participant_type.name, event.name);
    let linkedin_add_url = format!(
        "https://www.linkedin.com/profile/add?startTask=CERTIFICATION_NAME&name={}&organizationName={}&issueYear={}&issueMonth={}&certUrl={}",
        percent_encoding::percent_encode(linkedin_name.as_bytes(), NON_ALPHANUMERIC),
        percent_encoding::percent_encode(event.org_name.as_bytes(), NON_ALPHANUMERIC),
        event.event_date.map(|d| d.year()).unwrap_or(chrono::Utc::now().year()),
        event.event_date.map(|d| d.month()).unwrap_or(chrono::Utc::now().month()),
        percent_encoding::percent_encode(verify_url.as_bytes(), NON_ALPHANUMERIC),
    );

    let kind_str = match kind {
        TemplateKind::Badge => "Badge",
        TemplateKind::Certificate => "Certificate",
    };

    let first_name = participant.name.split_whitespace().next().unwrap_or(&participant.name);

    let preview_url = format!(
        "{}/api/public/verify/asset/{}",
        config.public_base_url.trim_end_matches('/'),
        participant.short_code
    );

    let html_body = format!(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
        </head>
        <body style="margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background-color: #f3f4f6;">
            <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width: 600px; margin: 0 auto; padding: 20px;">
                <tr>
                    <td style="background: #ffffff; border-radius: 12px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.1);">
                        <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
                            <tr>
                                <td style="background: linear-gradient(135deg, #4f46e5 0%%, #7c3aed 100%%); padding: 30px 20px; text-align: center;">
                                    <h1 style="margin: 0; color: #ffffff; font-size: 24px; font-weight: 700;">Your {} for {} 🎉</h1>
                                </td>
                            </tr>
                            <tr>
                                <td style="padding: 30px 30px 10px;">
                                    <p style="margin: 0 0 16px; font-size: 16px; color: #1f2937;">Hi {},</p>
                                    <p style="margin: 0 0 24px; font-size: 16px; color: #1f2937; line-height: 1.6;">
                                        Congratulations on your participation as a <strong>{}</strong> at <strong>{}</strong>!
                                        Your official {} is ready.
                                    </p>
                                </td>
                            </tr>
                            <tr>
                                <td style="padding: 0 30px 20px; text-align: center;">
                                    <img src="{}" alt="{} preview" style="max-width: 100%%; height: auto; border-radius: 8px; border: 1px solid #e5e7eb;">
                                </td>
                            </tr>
                            <tr>
                                <td style="padding: 0 30px 30px;">
                                    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom: 16px;">
                                        <tr>
                                            <td style="padding: 12px 24px; background: #4f46e5; border-radius: 8px; text-align: center;">
                                                <a href="{}" style="color: #ffffff; text-decoration: none; font-weight: 600; font-size: 16px; display: block;">Download {}</a>
                                            </td>
                                        </tr>
                                    </table>
                                    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom: 16px;">
                                        <tr>
                                            <td style="padding: 12px 24px; background: #e0e7ff; border-radius: 8px; text-align: center;">
                                                <a href="{}" style="color: #4f46e5; text-decoration: none; font-weight: 600; font-size: 16px; display: block;" target="_blank" rel="noopener noreferrer">Add to LinkedIn</a>
                                            </td>
                                        </tr>
                                    </table>
                                    <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
                                        <tr>
                                            <td style="padding: 12px 24px; background: #ffffff; border: 1px solid #4f46e5; border-radius: 8px; text-align: center;">
                                                <a href="{}" style="color: #4f46e5; text-decoration: none; font-weight: 600; font-size: 16px; display: block;" target="_blank" rel="noopener noreferrer">View & Verify Online</a>
                                            </td>
                                        </tr>
                                    </table>
                                </td>
                            </tr>
                            <tr>
                                <td style="padding: 20px 30px; background: #f9fafb; border-top: 1px solid #e5e7eb; text-align: center;">
                                    <p style="margin: 0; font-size: 12px; color: #9ca3af;">Issued by {} | <a href="{}" style="color: #4f46e5;">Verify at {}</a></p>
                                </td>
                            </tr>
                        </table>
                    </td>
                </tr>
            </table>
        </body>
        </html>
        "#,
        kind_str, event.name, first_name, participant_type.name, event.name, kind_str.to_lowercase(),
        preview_url, kind_str.to_lowercase(),
        download_url, kind_str.to_lowercase(),
        linkedin_add_url,
        verify_url,
        event.org_name, verify_url, verify_url
    );

    let email = Message::builder()
        .from(config.smtp_from.parse::<Mailbox>().map_err(|e| CoreError::Email(e.to_string()))?)
        .to(
            format!("{} <{}>", participant.name, participant.email)
                .parse::<Mailbox>()
                .map_err(|e| CoreError::Email(e.to_string()))?,
        )
        .subject(format!("Your {} for {} 🎉", kind_str, event.name))
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(header::ContentType::TEXT_HTML)
                        .body(html_body),
                ),
        )
        .map_err(|e| CoreError::Email(e.to_string()))?;

    let mailer = if config.smtp_user.is_empty() {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(config.smtp_host.clone())
            .port(config.smtp_port)
            .build()
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
            .map_err(|e| CoreError::Email(e.to_string()))?
            .port(config.smtp_port)
            .credentials(lettre::transport::smtp::authentication::Credentials::new(
                config.smtp_user.clone(),
                config.smtp_pass.clone(),
            ))
            .build()
    };

    mailer.send(email).await.map_err(|e| CoreError::Email(e.to_string()))?;

    Ok(())
}