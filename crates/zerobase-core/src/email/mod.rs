//! Email service abstraction and template engine.
//!
//! Defines the [`EmailService`] trait used by services to send transactional
//! emails (verification, password reset, OTP, etc.). Concrete implementations
//! live in `zerobase-auth` (SMTP via `lettre`).
//!
//! The [`templates`] module provides a type-safe template engine for rendering
//! all transactional email types with both HTML and plain-text variants.

pub mod templates;

use crate::error::ZerobaseError;

/// A single email message to be sent.
#[derive(Debug, Clone)]
pub struct EmailMessage {
    /// Recipient email address.
    pub to: String,
    /// Email subject line.
    pub subject: String,
    /// Plain-text body.
    pub body_text: String,
    /// Optional HTML body.
    pub body_html: Option<String>,
}

/// Abstraction over email sending.
///
/// The record service uses this trait to send verification emails, password
/// reset emails, OTP codes, etc. This keeps the core crate free from direct
/// SMTP/transport dependencies while allowing the auth crate to provide the
/// concrete implementation.
pub trait EmailService: Send + Sync {
    /// Send an email message. Returns an error if delivery fails.
    fn send(&self, message: &EmailMessage) -> Result<(), ZerobaseError>;
}

/// A no-op email service used when SMTP is not configured.
///
/// Returns an error on every `send` call, making it clear to callers that
/// email delivery is unavailable.
pub struct NoopEmailService;

impl EmailService for NoopEmailService {
    fn send(&self, _message: &EmailMessage) -> Result<(), ZerobaseError> {
        Err(ZerobaseError::internal(
            "email delivery is not configured — enable SMTP in settings",
        ))
    }
}

/// A no-op email service that records sent messages. **Only for testing.**
#[cfg(test)]
pub struct MockEmailService {
    /// All messages "sent" via this service.
    pub sent: std::sync::Mutex<Vec<EmailMessage>>,
}

#[cfg(test)]
impl MockEmailService {
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
impl EmailService for MockEmailService {
    fn send(&self, message: &EmailMessage) -> Result<(), ZerobaseError> {
        self.sent.lock().unwrap().push(message.clone());
        Ok(())
    }
}
