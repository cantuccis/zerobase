//! Type-safe email template engine for transactional emails.
//!
//! Provides [`EmailTemplateEngine`] which renders all transactional email types
//! (verification, password reset, email change, OTP) into [`EmailMessage`]
//! values with both HTML and plain-text bodies.
//!
//! ## Design
//!
//! Templates are defined as Rust code rather than external files, ensuring
//! compile-time safety and avoiding runtime file I/O. The engine is configured
//! with an `app_name` (used in subjects and bodies) and produces ready-to-send
//! [`EmailMessage`] values.
//!
//! Each template type has a dedicated context struct that holds all the
//! dynamic data needed for rendering. This prevents parameter mismatches
//! and makes templates self-documenting.

use super::EmailMessage;

/// Escape a string for safe inclusion in HTML.
///
/// This prevents XSS in email templates by encoding `<`, `>`, `&`, `"`, and `'`.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// Engine for rendering transactional email templates.
///
/// Configured once with the application name and reused across the
/// application lifetime.
#[derive(Debug, Clone)]
pub struct EmailTemplateEngine {
    /// Application name shown in email subjects and footers.
    app_name: String,
}

impl EmailTemplateEngine {
    /// Create a new template engine with the given application name.
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }

    /// Render a verification email.
    pub fn verification(&self, ctx: &VerificationContext) -> EmailMessage {
        let subject = format!("Verify your email - {}", self.app_name);

        let body_text = format!(
            "Hello,\n\n\
             Please verify your email address for {app_name} by visiting the link below:\n\n\
             {url}\n\n\
             This link expires in {expiry}.\n\n\
             If you did not create an account, please ignore this email.\n\n\
             — {app_name}",
            app_name = self.app_name,
            url = ctx.verification_url,
            expiry = ctx.expiry_text,
        );

        // HTML-escape all dynamic values to prevent XSS in email clients.
        let app_name_h = html_escape(&self.app_name);
        let url_h = html_escape(&ctx.verification_url);
        let expiry_h = html_escape(&ctx.expiry_text);

        let body_html = format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
  <h2 style="color: #2563eb;">{app_name}</h2>
  <p>Hello,</p>
  <p>Please verify your email address by clicking the button below:</p>
  <p style="text-align: center; margin: 30px 0;">
    <a href="{url}" style="background-color: #2563eb; color: #ffffff; padding: 12px 24px; text-decoration: none; border-radius: 6px; font-weight: 600; display: inline-block;">Verify Email</a>
  </p>
  <p style="font-size: 14px; color: #666;">Or copy and paste this URL into your browser:</p>
  <p style="font-size: 14px; color: #666; word-break: break-all;">{url}</p>
  <p style="font-size: 14px; color: #666;">This link expires in {expiry}.</p>
  <hr style="border: none; border-top: 1px solid #eee; margin: 30px 0;">
  <p style="font-size: 12px; color: #999;">If you did not create an account, please ignore this email.</p>
</body>
</html>"#,
            app_name = app_name_h,
            url = url_h,
            expiry = expiry_h,
        );

        EmailMessage {
            to: ctx.to.clone(),
            subject,
            body_text,
            body_html: Some(body_html),
        }
    }

    /// Render a password reset email.
    pub fn password_reset(&self, ctx: &PasswordResetContext) -> EmailMessage {
        let subject = format!("Reset your password - {}", self.app_name);

        let body_text = format!(
            "Hello,\n\n\
             You requested a password reset for your {app_name} account.\n\n\
             Visit the link below to set a new password:\n\n\
             {url}\n\n\
             This link expires in {expiry}.\n\n\
             If you did not request this, please ignore this email. Your password will remain unchanged.\n\n\
             — {app_name}",
            app_name = self.app_name,
            url = ctx.reset_url,
            expiry = ctx.expiry_text,
        );

        let app_name_h = html_escape(&self.app_name);
        let url_h = html_escape(&ctx.reset_url);
        let expiry_h = html_escape(&ctx.expiry_text);

        let body_html = format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
  <h2 style="color: #2563eb;">{app_name}</h2>
  <p>Hello,</p>
  <p>You requested a password reset for your account. Click the button below to set a new password:</p>
  <p style="text-align: center; margin: 30px 0;">
    <a href="{url}" style="background-color: #2563eb; color: #ffffff; padding: 12px 24px; text-decoration: none; border-radius: 6px; font-weight: 600; display: inline-block;">Reset Password</a>
  </p>
  <p style="font-size: 14px; color: #666;">Or copy and paste this URL into your browser:</p>
  <p style="font-size: 14px; color: #666; word-break: break-all;">{url}</p>
  <p style="font-size: 14px; color: #666;">This link expires in {expiry}.</p>
  <hr style="border: none; border-top: 1px solid #eee; margin: 30px 0;">
  <p style="font-size: 12px; color: #999;">If you did not request this, please ignore this email. Your password will remain unchanged.</p>
</body>
</html>"#,
            app_name = app_name_h,
            url = url_h,
            expiry = expiry_h,
        );

        EmailMessage {
            to: ctx.to.clone(),
            subject,
            body_text,
            body_html: Some(body_html),
        }
    }

    /// Render an email change confirmation email.
    ///
    /// This email is sent to the **new** email address for confirmation.
    pub fn email_change(&self, ctx: &EmailChangeContext) -> EmailMessage {
        let subject = format!("Confirm your new email - {}", self.app_name);

        let body_text = format!(
            "Hello,\n\n\
             You requested to change your email address for your {app_name} account.\n\n\
             Please confirm by visiting the link below:\n\n\
             {url}\n\n\
             This link expires in {expiry}.\n\n\
             If you did not request this change, please ignore this email.\n\n\
             — {app_name}",
            app_name = self.app_name,
            url = ctx.confirm_url,
            expiry = ctx.expiry_text,
        );

        let app_name_h = html_escape(&self.app_name);
        let url_h = html_escape(&ctx.confirm_url);
        let expiry_h = html_escape(&ctx.expiry_text);

        let body_html = format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
  <h2 style="color: #2563eb;">{app_name}</h2>
  <p>Hello,</p>
  <p>You requested to change your email address. Click the button below to confirm:</p>
  <p style="text-align: center; margin: 30px 0;">
    <a href="{url}" style="background-color: #2563eb; color: #ffffff; padding: 12px 24px; text-decoration: none; border-radius: 6px; font-weight: 600; display: inline-block;">Confirm Email Change</a>
  </p>
  <p style="font-size: 14px; color: #666;">Or copy and paste this URL into your browser:</p>
  <p style="font-size: 14px; color: #666; word-break: break-all;">{url}</p>
  <p style="font-size: 14px; color: #666;">This link expires in {expiry}.</p>
  <hr style="border: none; border-top: 1px solid #eee; margin: 30px 0;">
  <p style="font-size: 12px; color: #999;">If you did not request this change, please ignore this email.</p>
</body>
</html>"#,
            app_name = app_name_h,
            url = url_h,
            expiry = expiry_h,
        );

        EmailMessage {
            to: ctx.to.clone(),
            subject,
            body_text,
            body_html: Some(body_html),
        }
    }

    /// Render an OTP (One-Time Password) email.
    pub fn otp(&self, ctx: &OtpContext) -> EmailMessage {
        let subject = format!("Your verification code - {}", self.app_name);

        let body_text = format!(
            "Hello,\n\n\
             Your {app_name} verification code is:\n\n\
             {code}\n\n\
             This code expires in {expiry}.\n\n\
             If you did not request this code, please ignore this email.\n\n\
             — {app_name}",
            app_name = self.app_name,
            code = ctx.otp_code,
            expiry = ctx.expiry_text,
        );

        let app_name_h = html_escape(&self.app_name);
        let code_h = html_escape(&ctx.otp_code);
        let expiry_h = html_escape(&ctx.expiry_text);

        let body_html = format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
  <h2 style="color: #2563eb;">{app_name}</h2>
  <p>Hello,</p>
  <p>Your verification code is:</p>
  <p style="text-align: center; margin: 30px 0;">
    <span style="font-family: monospace; font-size: 32px; font-weight: 700; letter-spacing: 0.3em; color: #2563eb; background: #f0f4ff; padding: 12px 24px; border-radius: 8px; display: inline-block;">{code}</span>
  </p>
  <p style="font-size: 14px; color: #666;">This code expires in {expiry}.</p>
  <hr style="border: none; border-top: 1px solid #eee; margin: 30px 0;">
  <p style="font-size: 12px; color: #999;">If you did not request this code, please ignore this email.</p>
</body>
</html>"#,
            app_name = app_name_h,
            code = code_h,
            expiry = expiry_h,
        );

        EmailMessage {
            to: ctx.to.clone(),
            subject,
            body_text,
            body_html: Some(body_html),
        }
    }
}

// ── Template context types ──────────────────────────────────────────────────

/// Context for rendering a verification email.
#[derive(Debug, Clone)]
pub struct VerificationContext {
    /// Recipient email address.
    pub to: String,
    /// Full verification URL including the token.
    pub verification_url: String,
    /// Human-readable expiry text (e.g. "7 days").
    pub expiry_text: String,
}

/// Context for rendering a password reset email.
#[derive(Debug, Clone)]
pub struct PasswordResetContext {
    /// Recipient email address.
    pub to: String,
    /// Full password reset URL including the token.
    pub reset_url: String,
    /// Human-readable expiry text (e.g. "1 hour").
    pub expiry_text: String,
}

/// Context for rendering an email change confirmation email.
#[derive(Debug, Clone)]
pub struct EmailChangeContext {
    /// Recipient email address (the **new** address).
    pub to: String,
    /// Full confirmation URL including the token.
    pub confirm_url: String,
    /// Human-readable expiry text (e.g. "1 hour").
    pub expiry_text: String,
}

/// Context for rendering an OTP email.
#[derive(Debug, Clone)]
pub struct OtpContext {
    /// Recipient email address.
    pub to: String,
    /// The OTP code (e.g. "847293").
    pub otp_code: String,
    /// Human-readable expiry text (e.g. "5 minutes").
    pub expiry_text: String,
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> EmailTemplateEngine {
        EmailTemplateEngine::new("Zerobase")
    }

    // ── Verification template ───────────────────────────────────────────

    #[test]
    fn verification_email_has_correct_subject() {
        let msg = engine().verification(&VerificationContext {
            to: "user@example.com".into(),
            verification_url: "https://app.test/verify?token=abc".into(),
            expiry_text: "7 days".into(),
        });
        assert_eq!(msg.subject, "Verify your email - Zerobase");
    }

    #[test]
    fn verification_email_sets_recipient() {
        let msg = engine().verification(&VerificationContext {
            to: "user@example.com".into(),
            verification_url: "https://app.test/verify?token=abc".into(),
            expiry_text: "7 days".into(),
        });
        assert_eq!(msg.to, "user@example.com");
    }

    #[test]
    fn verification_email_text_contains_url() {
        let msg = engine().verification(&VerificationContext {
            to: "user@example.com".into(),
            verification_url: "https://app.test/verify?token=abc".into(),
            expiry_text: "7 days".into(),
        });
        assert!(msg.body_text.contains("https://app.test/verify?token=abc"));
    }

    #[test]
    fn verification_email_text_contains_expiry() {
        let msg = engine().verification(&VerificationContext {
            to: "user@example.com".into(),
            verification_url: "https://app.test/verify?token=abc".into(),
            expiry_text: "7 days".into(),
        });
        assert!(msg.body_text.contains("7 days"));
    }

    #[test]
    fn verification_email_text_contains_app_name() {
        let msg = engine().verification(&VerificationContext {
            to: "user@example.com".into(),
            verification_url: "https://app.test/verify?token=abc".into(),
            expiry_text: "7 days".into(),
        });
        assert!(msg.body_text.contains("Zerobase"));
    }

    #[test]
    fn verification_email_has_html_body() {
        let msg = engine().verification(&VerificationContext {
            to: "user@example.com".into(),
            verification_url: "https://app.test/verify?token=abc".into(),
            expiry_text: "7 days".into(),
        });
        let html = msg.body_html.as_ref().expect("should have HTML body");
        assert!(html.contains("Verify Email"));
        assert!(html.contains("https://app.test/verify?token=abc"));
        assert!(html.contains("7 days"));
    }

    #[test]
    fn verification_email_html_is_valid_structure() {
        let msg = engine().verification(&VerificationContext {
            to: "user@example.com".into(),
            verification_url: "https://app.test/verify?token=abc".into(),
            expiry_text: "7 days".into(),
        });
        let html = msg.body_html.as_ref().unwrap();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<body"));
        assert!(html.contains("</body>"));
    }

    // ── Password reset template ─────────────────────────────────────────

    #[test]
    fn password_reset_email_has_correct_subject() {
        let msg = engine().password_reset(&PasswordResetContext {
            to: "user@example.com".into(),
            reset_url: "https://app.test/reset?token=xyz".into(),
            expiry_text: "1 hour".into(),
        });
        assert_eq!(msg.subject, "Reset your password - Zerobase");
    }

    #[test]
    fn password_reset_email_text_contains_url() {
        let msg = engine().password_reset(&PasswordResetContext {
            to: "user@example.com".into(),
            reset_url: "https://app.test/reset?token=xyz".into(),
            expiry_text: "1 hour".into(),
        });
        assert!(msg.body_text.contains("https://app.test/reset?token=xyz"));
    }

    #[test]
    fn password_reset_email_text_contains_expiry() {
        let msg = engine().password_reset(&PasswordResetContext {
            to: "user@example.com".into(),
            reset_url: "https://app.test/reset?token=xyz".into(),
            expiry_text: "1 hour".into(),
        });
        assert!(msg.body_text.contains("1 hour"));
    }

    #[test]
    fn password_reset_email_has_html_with_button() {
        let msg = engine().password_reset(&PasswordResetContext {
            to: "user@example.com".into(),
            reset_url: "https://app.test/reset?token=xyz".into(),
            expiry_text: "1 hour".into(),
        });
        let html = msg.body_html.as_ref().unwrap();
        assert!(html.contains("Reset Password"));
        assert!(html.contains("https://app.test/reset?token=xyz"));
    }

    #[test]
    fn password_reset_email_text_warns_about_unsolicited() {
        let msg = engine().password_reset(&PasswordResetContext {
            to: "user@example.com".into(),
            reset_url: "https://app.test/reset?token=xyz".into(),
            expiry_text: "1 hour".into(),
        });
        assert!(msg.body_text.contains("did not request"));
        assert!(msg.body_text.contains("remain unchanged"));
    }

    // ── Email change template ───────────────────────────────────────────

    #[test]
    fn email_change_has_correct_subject() {
        let msg = engine().email_change(&EmailChangeContext {
            to: "new@example.com".into(),
            confirm_url: "https://app.test/confirm?token=chg".into(),
            expiry_text: "1 hour".into(),
        });
        assert_eq!(msg.subject, "Confirm your new email - Zerobase");
    }

    #[test]
    fn email_change_text_contains_url() {
        let msg = engine().email_change(&EmailChangeContext {
            to: "new@example.com".into(),
            confirm_url: "https://app.test/confirm?token=chg".into(),
            expiry_text: "1 hour".into(),
        });
        assert!(msg.body_text.contains("https://app.test/confirm?token=chg"));
    }

    #[test]
    fn email_change_html_has_button() {
        let msg = engine().email_change(&EmailChangeContext {
            to: "new@example.com".into(),
            confirm_url: "https://app.test/confirm?token=chg".into(),
            expiry_text: "1 hour".into(),
        });
        let html = msg.body_html.as_ref().unwrap();
        assert!(html.contains("Confirm Email Change"));
    }

    #[test]
    fn email_change_sends_to_new_address() {
        let msg = engine().email_change(&EmailChangeContext {
            to: "new@example.com".into(),
            confirm_url: "https://app.test/confirm?token=chg".into(),
            expiry_text: "1 hour".into(),
        });
        assert_eq!(msg.to, "new@example.com");
    }

    // ── OTP template ────────────────────────────────────────────────────

    #[test]
    fn otp_email_has_correct_subject() {
        let msg = engine().otp(&OtpContext {
            to: "user@example.com".into(),
            otp_code: "847293".into(),
            expiry_text: "5 minutes".into(),
        });
        assert_eq!(msg.subject, "Your verification code - Zerobase");
    }

    #[test]
    fn otp_email_text_contains_code() {
        let msg = engine().otp(&OtpContext {
            to: "user@example.com".into(),
            otp_code: "847293".into(),
            expiry_text: "5 minutes".into(),
        });
        assert!(msg.body_text.contains("847293"));
    }

    #[test]
    fn otp_email_text_contains_expiry() {
        let msg = engine().otp(&OtpContext {
            to: "user@example.com".into(),
            otp_code: "847293".into(),
            expiry_text: "5 minutes".into(),
        });
        assert!(msg.body_text.contains("5 minutes"));
    }

    #[test]
    fn otp_email_html_contains_styled_code() {
        let msg = engine().otp(&OtpContext {
            to: "user@example.com".into(),
            otp_code: "847293".into(),
            expiry_text: "5 minutes".into(),
        });
        let html = msg.body_html.as_ref().unwrap();
        assert!(html.contains("847293"));
        assert!(html.contains("letter-spacing"));
        assert!(html.contains("monospace"));
    }

    // ── Custom app name ─────────────────────────────────────────────────

    #[test]
    fn custom_app_name_appears_in_all_templates() {
        let engine = EmailTemplateEngine::new("MyCustomApp");

        let v = engine.verification(&VerificationContext {
            to: "u@x.com".into(),
            verification_url: "https://x.com".into(),
            expiry_text: "1d".into(),
        });
        assert!(v.subject.contains("MyCustomApp"));
        assert!(v.body_text.contains("MyCustomApp"));

        let p = engine.password_reset(&PasswordResetContext {
            to: "u@x.com".into(),
            reset_url: "https://x.com".into(),
            expiry_text: "1h".into(),
        });
        assert!(p.subject.contains("MyCustomApp"));
        assert!(p.body_text.contains("MyCustomApp"));

        let e = engine.email_change(&EmailChangeContext {
            to: "u@x.com".into(),
            confirm_url: "https://x.com".into(),
            expiry_text: "1h".into(),
        });
        assert!(e.subject.contains("MyCustomApp"));
        assert!(e.body_text.contains("MyCustomApp"));

        let o = engine.otp(&OtpContext {
            to: "u@x.com".into(),
            otp_code: "123456".into(),
            expiry_text: "5m".into(),
        });
        assert!(o.subject.contains("MyCustomApp"));
        assert!(o.body_text.contains("MyCustomApp"));
    }

    // ── HTML safety ─────────────────────────────────────────────────────

    #[test]
    fn all_templates_produce_html_body() {
        let engine = engine();

        let v = engine.verification(&VerificationContext {
            to: "u@x.com".into(),
            verification_url: "https://x.com".into(),
            expiry_text: "1d".into(),
        });
        assert!(v.body_html.is_some());

        let p = engine.password_reset(&PasswordResetContext {
            to: "u@x.com".into(),
            reset_url: "https://x.com".into(),
            expiry_text: "1h".into(),
        });
        assert!(p.body_html.is_some());

        let e = engine.email_change(&EmailChangeContext {
            to: "u@x.com".into(),
            confirm_url: "https://x.com".into(),
            expiry_text: "1h".into(),
        });
        assert!(e.body_html.is_some());

        let o = engine.otp(&OtpContext {
            to: "u@x.com".into(),
            otp_code: "123456".into(),
            expiry_text: "5m".into(),
        });
        assert!(o.body_html.is_some());
    }

    #[test]
    fn all_templates_have_nonempty_text_body() {
        let engine = engine();

        let v = engine.verification(&VerificationContext {
            to: "u@x.com".into(),
            verification_url: "https://x.com".into(),
            expiry_text: "1d".into(),
        });
        assert!(!v.body_text.is_empty());

        let p = engine.password_reset(&PasswordResetContext {
            to: "u@x.com".into(),
            reset_url: "https://x.com".into(),
            expiry_text: "1h".into(),
        });
        assert!(!p.body_text.is_empty());

        let e = engine.email_change(&EmailChangeContext {
            to: "u@x.com".into(),
            confirm_url: "https://x.com".into(),
            expiry_text: "1h".into(),
        });
        assert!(!e.body_text.is_empty());

        let o = engine.otp(&OtpContext {
            to: "u@x.com".into(),
            otp_code: "123456".into(),
            expiry_text: "5m".into(),
        });
        assert!(!o.body_text.is_empty());
    }

    // ── XSS prevention (html_escape) ───────────────────────────────────

    #[test]
    fn html_escape_encodes_all_dangerous_chars() {
        assert_eq!(html_escape("&"), "&amp;");
        assert_eq!(html_escape("<"), "&lt;");
        assert_eq!(html_escape(">"), "&gt;");
        assert_eq!(html_escape("\""), "&quot;");
        assert_eq!(html_escape("'"), "&#x27;");
        assert_eq!(
            html_escape("<script>alert('xss')</script>"),
            "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;"
        );
    }

    #[test]
    fn verification_html_escapes_malicious_app_name() {
        let engine = EmailTemplateEngine::new("<script>alert(1)</script>");
        let msg = engine.verification(&VerificationContext {
            to: "u@x.com".into(),
            verification_url: "https://x.com".into(),
            expiry_text: "1d".into(),
        });
        let html = msg.body_html.as_ref().unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn otp_html_escapes_malicious_code() {
        let engine = engine();
        let msg = engine.otp(&OtpContext {
            to: "u@x.com".into(),
            otp_code: "<img src=x onerror=alert(1)>".into(),
            expiry_text: "5m".into(),
        });
        let html = msg.body_html.as_ref().unwrap();
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;img"));
    }

    #[test]
    fn password_reset_html_escapes_url() {
        let engine = engine();
        let msg = engine.password_reset(&PasswordResetContext {
            to: "u@x.com".into(),
            reset_url: "javascript:alert('xss')".into(),
            expiry_text: "1h".into(),
        });
        let html = msg.body_html.as_ref().unwrap();
        // The URL should be escaped in the HTML (quotes escaped)
        assert!(html.contains("javascript:alert(&#x27;xss&#x27;)"));
    }
}
