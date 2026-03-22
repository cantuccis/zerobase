//! CLI argument parsing for the Zerobase server binary.
//!
//! Uses `clap` derive macros to define the command-line interface:
//!
//! - `serve`    — start the HTTP server
//! - `migrate`  — run pending database migrations
//! - `superuser` — create / update / delete superuser accounts
//! - (no subcommand) — prints version by default
//!
//! CLI flags override config-file and environment-variable settings,
//! giving the user a consistent precedence chain:
//! defaults < config file < env vars < CLI flags.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Zerobase — single-binary Backend-as-a-Service.
#[derive(Parser, Debug)]
#[command(
    name = "zerobase",
    version = env!("CARGO_PKG_VERSION"),
    about = "Single-binary Backend-as-a-Service built in Rust",
    long_about = "Zerobase is a BaaS tool that provides an embedded SQLite database, \
                  auto-generated REST API, built-in authentication, realtime subscriptions, \
                  file storage, and an admin dashboard — all in a single binary."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the HTTP server.
    Serve(ServeArgs),

    /// Run pending database migrations.
    Migrate(MigrateArgs),

    /// Manage superuser accounts.
    Superuser(SuperuserArgs),

    /// Print version information and exit.
    Version,
}

/// Arguments for the `serve` subcommand.
#[derive(Parser, Debug)]
pub struct ServeArgs {
    /// Host address to bind to (overrides config).
    #[arg(long)]
    pub host: Option<String>,

    /// Port to listen on (overrides config).
    #[arg(long, short)]
    pub port: Option<u16>,

    /// Path to the data directory containing the SQLite database.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    /// Log format: `json` or `pretty`.
    #[arg(long)]
    pub log_format: Option<String>,
}

/// Arguments for the `migrate` subcommand.
#[derive(Parser, Debug)]
pub struct MigrateArgs {
    /// Path to the data directory containing the SQLite database.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
}

/// Arguments for the `superuser` subcommand.
#[derive(Parser, Debug)]
pub struct SuperuserArgs {
    #[command(subcommand)]
    pub action: SuperuserAction,
}

/// Superuser management sub-subcommands.
#[derive(Subcommand, Debug)]
pub enum SuperuserAction {
    /// Create a new superuser account.
    Create(SuperuserCreateArgs),

    /// Update an existing superuser's email or password.
    Update(SuperuserUpdateArgs),

    /// Delete a superuser account by email.
    Delete(SuperuserDeleteArgs),

    /// List all superuser accounts.
    List,
}

/// Arguments for `superuser create`.
#[derive(Parser, Debug)]
pub struct SuperuserCreateArgs {
    /// Superuser email address.
    #[arg(long)]
    pub email: String,

    /// Superuser password (min 8 characters).
    #[arg(long)]
    pub password: String,
}

/// Arguments for `superuser update`.
#[derive(Parser, Debug)]
pub struct SuperuserUpdateArgs {
    /// Current email of the superuser to update.
    #[arg(long)]
    pub email: String,

    /// New email address (optional).
    #[arg(long)]
    pub new_email: Option<String>,

    /// New password (optional, min 8 characters).
    #[arg(long)]
    pub new_password: Option<String>,
}

/// Arguments for `superuser delete`.
#[derive(Parser, Debug)]
pub struct SuperuserDeleteArgs {
    /// Email of the superuser to delete.
    #[arg(long)]
    pub email: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    // ── Basic parsing sanity ──────────────────────────────────────────

    #[test]
    fn cli_parses_no_args() {
        let cli = Cli::try_parse_from(["zerobase"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_parses_version_subcommand() {
        let cli = Cli::try_parse_from(["zerobase", "version"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Version)));
    }

    // ── Serve ─────────────────────────────────────────────────────────

    #[test]
    fn serve_no_args() {
        let cli = Cli::try_parse_from(["zerobase", "serve"]).unwrap();
        match cli.command {
            Some(Command::Serve(args)) => {
                assert!(args.host.is_none());
                assert!(args.port.is_none());
                assert!(args.data_dir.is_none());
                assert!(args.log_format.is_none());
            }
            other => panic!("expected Serve, got {other:?}"),
        }
    }

    #[test]
    fn serve_with_all_flags() {
        let cli = Cli::try_parse_from([
            "zerobase",
            "serve",
            "--host",
            "0.0.0.0",
            "--port",
            "9090",
            "--data-dir",
            "/tmp/zb",
            "--log-format",
            "pretty",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Serve(args)) => {
                assert_eq!(args.host.as_deref(), Some("0.0.0.0"));
                assert_eq!(args.port, Some(9090));
                assert_eq!(args.data_dir.as_deref(), Some(std::path::Path::new("/tmp/zb")));
                assert_eq!(args.log_format.as_deref(), Some("pretty"));
            }
            other => panic!("expected Serve, got {other:?}"),
        }
    }

    #[test]
    fn serve_short_port_flag() {
        let cli = Cli::try_parse_from(["zerobase", "serve", "-p", "3000"]).unwrap();
        match cli.command {
            Some(Command::Serve(args)) => assert_eq!(args.port, Some(3000)),
            other => panic!("expected Serve, got {other:?}"),
        }
    }

    #[test]
    fn serve_rejects_invalid_port() {
        let result = Cli::try_parse_from(["zerobase", "serve", "--port", "not_a_number"]);
        assert!(result.is_err());
    }

    // ── Migrate ───────────────────────────────────────────────────────

    #[test]
    fn migrate_no_args() {
        let cli = Cli::try_parse_from(["zerobase", "migrate"]).unwrap();
        match cli.command {
            Some(Command::Migrate(args)) => assert!(args.data_dir.is_none()),
            other => panic!("expected Migrate, got {other:?}"),
        }
    }

    #[test]
    fn migrate_with_data_dir() {
        let cli =
            Cli::try_parse_from(["zerobase", "migrate", "--data-dir", "/var/zerobase"]).unwrap();
        match cli.command {
            Some(Command::Migrate(args)) => {
                assert_eq!(
                    args.data_dir.as_deref(),
                    Some(std::path::Path::new("/var/zerobase"))
                );
            }
            other => panic!("expected Migrate, got {other:?}"),
        }
    }

    // ── Superuser create ──────────────────────────────────────────────

    #[test]
    fn superuser_create() {
        let cli = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "create",
            "--email",
            "admin@test.com",
            "--password",
            "secret123",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Superuser(su)) => match su.action {
                SuperuserAction::Create(args) => {
                    assert_eq!(args.email, "admin@test.com");
                    assert_eq!(args.password, "secret123");
                }
                other => panic!("expected Create, got {other:?}"),
            },
            other => panic!("expected Superuser, got {other:?}"),
        }
    }

    #[test]
    fn superuser_create_requires_email() {
        let result = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "create",
            "--password",
            "secret123",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn superuser_create_requires_password() {
        let result = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "create",
            "--email",
            "admin@test.com",
        ]);
        assert!(result.is_err());
    }

    // ── Superuser update ──────────────────────────────────────────────

    #[test]
    fn superuser_update_new_email() {
        let cli = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "update",
            "--email",
            "old@test.com",
            "--new-email",
            "new@test.com",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Superuser(su)) => match su.action {
                SuperuserAction::Update(args) => {
                    assert_eq!(args.email, "old@test.com");
                    assert_eq!(args.new_email.as_deref(), Some("new@test.com"));
                    assert!(args.new_password.is_none());
                }
                other => panic!("expected Update, got {other:?}"),
            },
            other => panic!("expected Superuser, got {other:?}"),
        }
    }

    #[test]
    fn superuser_update_new_password() {
        let cli = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "update",
            "--email",
            "admin@test.com",
            "--new-password",
            "newpass123",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Superuser(su)) => match su.action {
                SuperuserAction::Update(args) => {
                    assert_eq!(args.email, "admin@test.com");
                    assert!(args.new_email.is_none());
                    assert_eq!(args.new_password.as_deref(), Some("newpass123"));
                }
                other => panic!("expected Update, got {other:?}"),
            },
            other => panic!("expected Superuser, got {other:?}"),
        }
    }

    #[test]
    fn superuser_update_both() {
        let cli = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "update",
            "--email",
            "admin@test.com",
            "--new-email",
            "new@test.com",
            "--new-password",
            "newpass123",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Superuser(su)) => match su.action {
                SuperuserAction::Update(args) => {
                    assert_eq!(args.email, "admin@test.com");
                    assert_eq!(args.new_email.as_deref(), Some("new@test.com"));
                    assert_eq!(args.new_password.as_deref(), Some("newpass123"));
                }
                other => panic!("expected Update, got {other:?}"),
            },
            other => panic!("expected Superuser, got {other:?}"),
        }
    }

    #[test]
    fn superuser_update_requires_email() {
        let result = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "update",
            "--new-email",
            "new@test.com",
        ]);
        assert!(result.is_err());
    }

    // ── Superuser delete ──────────────────────────────────────────────

    #[test]
    fn superuser_delete() {
        let cli = Cli::try_parse_from([
            "zerobase",
            "superuser",
            "delete",
            "--email",
            "admin@test.com",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Superuser(su)) => match su.action {
                SuperuserAction::Delete(args) => {
                    assert_eq!(args.email, "admin@test.com");
                }
                other => panic!("expected Delete, got {other:?}"),
            },
            other => panic!("expected Superuser, got {other:?}"),
        }
    }

    #[test]
    fn superuser_delete_requires_email() {
        let result = Cli::try_parse_from(["zerobase", "superuser", "delete"]);
        assert!(result.is_err());
    }

    // ── Superuser list ────────────────────────────────────────────────

    #[test]
    fn superuser_list() {
        let cli = Cli::try_parse_from(["zerobase", "superuser", "list"]).unwrap();
        match cli.command {
            Some(Command::Superuser(su)) => {
                assert!(matches!(su.action, SuperuserAction::List));
            }
            other => panic!("expected Superuser, got {other:?}"),
        }
    }

    // ── Help / debug rendering ────────────────────────────────────────

    #[test]
    fn cli_debug_assert() {
        // Validates the clap attributes at compile time.
        Cli::command().debug_assert();
    }

    #[test]
    fn unknown_subcommand_rejected() {
        let result = Cli::try_parse_from(["zerobase", "frobnicate"]);
        assert!(result.is_err());
    }

    #[test]
    fn version_flag_works() {
        // `--version` causes clap to return an error of kind DisplayVersion.
        let result = Cli::try_parse_from(["zerobase", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }
}
