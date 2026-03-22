//! Structured logging and tracing initialization.
//!
//! Provides a single entry point ([`init_tracing`]) that configures the global
//! tracing subscriber based on the desired [`LogFormat`].
//!
//! - **Json** — structured JSON, one object per line.  Ideal for log
//!   aggregators (ELK, Datadog, CloudWatch).
//! - **Pretty** — coloured, human-readable output for local development.
//!
//! Both formats respect the `RUST_LOG` environment variable for filtering.
//! The default filter is `info` for Zerobase crates and `warn` for everything
//! else.

use serde::Deserialize;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Selects the log output format.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Structured JSON — one object per line (default for production).
    #[default]
    Json,
    /// Human-readable pretty-printed output (default for development).
    Pretty,
}

/// Build the default [`EnvFilter`].
///
/// Respects `RUST_LOG` if set; otherwise defaults to debug for zerobase crates,
/// debug for tower_http, and info for everything else.
pub fn default_env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "zerobase_core=debug,\
             zerobase_db=debug,\
             zerobase_auth=debug,\
             zerobase_api=debug,\
             zerobase_files=debug,\
             zerobase_admin=debug,\
             zerobase_server=debug,\
             tower_http=debug,\
             info",
        )
    })
}

/// Initialize the global tracing subscriber.
///
/// This must be called **once** at application startup.  Calling it a second
/// time will panic (tracing only allows a single global subscriber).
///
/// # Examples
///
/// ```rust,no_run
/// use zerobase_core::telemetry::{init_tracing, LogFormat};
///
/// init_tracing(LogFormat::Pretty);
/// ```
pub fn init_tracing(format: LogFormat) {
    let env_filter = default_env_filter();

    match format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_target(true)
                        .with_thread_ids(true)
                        .with_span_list(true),
                )
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .pretty()
                        .with_target(true)
                        .with_thread_ids(false),
                )
                .init();
        }
    }
}

/// Initialize a JSON tracing subscriber that writes to the given writer.
///
/// Useful for tests that want to capture and inspect structured log output.
/// The returned guard must be held for the duration of the test — dropping it
/// unsets the subscriber.
///
/// **Note:** because tracing uses a global subscriber, tests using this helper
/// must not run in parallel. Use `#[serial_test::serial]` or similar.
pub fn init_json_tracing_to_writer<W>(
    writer: W,
    env_filter: EnvFilter,
) -> tracing::subscriber::DefaultGuard
where
    W: for<'w> tracing_subscriber::fmt::MakeWriter<'w> + Send + Sync + 'static,
{
    let subscriber = tracing_subscriber::registry().with(env_filter).with(
        fmt::layer()
            .json()
            .with_target(true)
            .with_thread_ids(true)
            .with_span_list(true)
            .with_writer(writer),
    );

    tracing::subscriber::set_default(subscriber)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_deserializes_from_lowercase() {
        let json: LogFormat = serde_json::from_str(r#""json""#).unwrap();
        assert_eq!(json, LogFormat::Json);

        let pretty: LogFormat = serde_json::from_str(r#""pretty""#).unwrap();
        assert_eq!(pretty, LogFormat::Pretty);
    }

    #[test]
    fn log_format_default_is_json() {
        assert_eq!(LogFormat::default(), LogFormat::Json);
    }

    #[test]
    fn default_env_filter_does_not_panic() {
        let _filter = default_env_filter();
    }
}
