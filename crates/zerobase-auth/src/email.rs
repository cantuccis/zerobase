//! SMTP email service implementation using `lettre`.
//!
//! Provides [`SmtpEmailService`], the production implementation of the core
//! [`EmailService`] trait that sends transactional emails via an SMTP relay.

use lettre::message::{header::ContentType, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use tracing::{info, warn};

use zerobase_core::configuration::SmtpSettings;
use zerobase_core::email::{EmailMessage, EmailService};
use zerobase_core::error::ZerobaseError;

/// Production SMTP email service using `lettre`.
///
/// Connects to an SMTP server based on the application's SMTP settings.
/// Thread-safe and reusable across requests.
pub struct SmtpEmailService {
    transport: SmtpTransport,
    sender: Mailbox,
}

impl SmtpEmailService {
    /// Create a new SMTP email service from application settings.
    ///
    /// Returns `None` if SMTP is disabled in the settings.
    pub fn from_settings(settings: &SmtpSettings) -> Option<Self> {
        if !settings.enabled {
            return None;
        }

        let sender: Mailbox = if settings.sender_name.is_empty() {
            settings
                .sender_address
                .parse()
                .expect("invalid sender_address in SMTP settings")
        } else {
            format!("{} <{}>", settings.sender_name, settings.sender_address)
                .parse()
                .expect("invalid sender_name/sender_address in SMTP settings")
        };

        let creds = if !settings.username.is_empty() {
            Some(Credentials::new(
                settings.username.clone(),
                secrecy::ExposeSecret::expose_secret(&settings.password).to_string(),
            ))
        } else {
            None
        };

        // TLS mode selection:
        // - tls=true  + port 465 → implicit TLS (SmtpTransport::relay)
        // - tls=true  + port 587 → STARTTLS (SmtpTransport::starttls_relay)
        // - tls=true  + other    → implicit TLS (SmtpTransport::relay)
        // - tls=false            → plaintext (SmtpTransport::builder_dangerous)
        let transport = if settings.tls {
            if settings.port == 587 {
                // STARTTLS: start plaintext, upgrade to TLS
                let mut builder = SmtpTransport::starttls_relay(&settings.host)
                    .expect("invalid SMTP host")
                    .port(settings.port);
                if let Some(creds) = creds {
                    builder = builder.credentials(creds);
                }
                builder.build()
            } else {
                // Implicit TLS (typically port 465)
                let mut builder = SmtpTransport::relay(&settings.host)
                    .expect("invalid SMTP host")
                    .port(settings.port);
                if let Some(creds) = creds {
                    builder = builder.credentials(creds);
                }
                builder.build()
            }
        } else {
            let mut builder = SmtpTransport::builder_dangerous(&settings.host).port(settings.port);
            if let Some(creds) = creds {
                builder = builder.credentials(creds);
            }
            builder.build()
        };

        Some(Self { transport, sender })
    }

    /// Test the SMTP connection by issuing a NOOP command.
    ///
    /// Returns `Ok(())` if the server responds successfully, or an error
    /// describing the connection failure.
    pub fn test_connection(&self) -> Result<(), ZerobaseError> {
        self.transport
            .test_connection()
            .map_err(|e| ZerobaseError::internal(format!("SMTP connection test failed: {e}")))?;
        info!("SMTP connection test successful");
        Ok(())
    }
}

impl EmailService for SmtpEmailService {
    fn send(&self, message: &EmailMessage) -> Result<(), ZerobaseError> {
        let to_mailbox: Mailbox = message.to.parse().map_err(|e| {
            ZerobaseError::validation(format!("invalid recipient email '{}': {e}", message.to))
        })?;

        let email_builder = Message::builder()
            .from(self.sender.clone())
            .to(to_mailbox)
            .subject(&message.subject);

        let email = if let Some(html) = &message.body_html {
            email_builder
                .multipart(
                    MultiPart::alternative()
                        .singlepart(
                            SinglePart::builder()
                                .content_type(ContentType::TEXT_PLAIN)
                                .body(message.body_text.clone()),
                        )
                        .singlepart(
                            SinglePart::builder()
                                .content_type(ContentType::TEXT_HTML)
                                .body(html.clone()),
                        ),
                )
                .map_err(|e| ZerobaseError::internal(format!("failed to build email: {e}")))?
        } else {
            email_builder
                .body(message.body_text.clone())
                .map_err(|e| ZerobaseError::internal(format!("failed to build email: {e}")))?
        };

        self.transport.send(&email).map_err(|e| {
            warn!(error = %e, to = %message.to, "failed to send email");
            ZerobaseError::internal(format!("failed to send email: {e}"))
        })?;

        info!(to = %message.to, subject = %message.subject, "email sent successfully");
        Ok(())
    }
}

/// A mock email service for testing that records all sent messages.
///
/// Unlike the core `MockEmailService` (cfg-gated), this is always available
/// within the auth crate's test builds.
#[cfg(test)]
pub(crate) struct TestEmailService {
    pub sent: std::sync::Mutex<Vec<EmailMessage>>,
}

#[cfg(test)]
impl TestEmailService {
    pub fn new() -> Self {
        Self {
            sent: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn sent_messages(&self) -> Vec<EmailMessage> {
        self.sent.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl EmailService for TestEmailService {
    fn send(&self, message: &EmailMessage) -> Result<(), ZerobaseError> {
        self.sent.lock().unwrap().push(message.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    fn make_settings(enabled: bool, tls: bool, port: u16) -> SmtpSettings {
        SmtpSettings {
            enabled,
            host: "smtp.example.com".to_string(),
            port,
            username: "user".to_string(),
            password: SecretString::from("pass"),
            sender_address: "noreply@example.com".to_string(),
            sender_name: "Test App".to_string(),
            tls,
        }
    }

    #[test]
    fn from_settings_returns_none_when_disabled() {
        let settings = make_settings(false, true, 465);
        assert!(SmtpEmailService::from_settings(&settings).is_none());
    }

    #[test]
    fn from_settings_returns_some_when_enabled() {
        let settings = make_settings(true, true, 465);
        assert!(SmtpEmailService::from_settings(&settings).is_some());
    }

    #[test]
    fn from_settings_works_with_starttls_port() {
        let settings = make_settings(true, true, 587);
        assert!(SmtpEmailService::from_settings(&settings).is_some());
    }

    #[test]
    fn from_settings_works_without_tls() {
        let settings = make_settings(true, false, 25);
        assert!(SmtpEmailService::from_settings(&settings).is_some());
    }

    #[test]
    fn from_settings_works_without_sender_name() {
        let settings = SmtpSettings {
            sender_name: String::new(),
            ..make_settings(true, true, 465)
        };
        assert!(SmtpEmailService::from_settings(&settings).is_some());
    }

    #[test]
    fn from_settings_works_without_credentials() {
        let settings = SmtpSettings {
            username: String::new(),
            ..make_settings(true, true, 465)
        };
        assert!(SmtpEmailService::from_settings(&settings).is_some());
    }

    #[test]
    fn send_rejects_invalid_recipient_email() {
        let settings = make_settings(true, false, 25);
        let service = SmtpEmailService::from_settings(&settings).unwrap();

        let msg = EmailMessage {
            to: "not-an-email".to_string(),
            subject: "Test".to_string(),
            body_text: "Hello".to_string(),
            body_html: None,
        };

        let result = service.send(&msg);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("invalid recipient"));
    }

    #[test]
    fn test_email_service_records_messages() {
        let svc = TestEmailService::new();
        let msg = EmailMessage {
            to: "user@example.com".to_string(),
            subject: "Hi".to_string(),
            body_text: "Hello".to_string(),
            body_html: Some("<p>Hello</p>".to_string()),
        };
        svc.send(&msg).unwrap();

        let sent = svc.sent_messages();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "user@example.com");
        assert_eq!(sent[0].subject, "Hi");
        assert!(sent[0].body_html.is_some());
    }
}
