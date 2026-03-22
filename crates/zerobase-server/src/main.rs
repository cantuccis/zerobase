//! Zerobase — single-binary Backend-as-a-Service.
//!
//! This is the composition root that wires together all crates
//! and starts the HTTP server. The binary supports several subcommands
//! (see [`cli`] module):
//!
//! - `serve`     — start the HTTP server
//! - `migrate`   — run pending database migrations
//! - `superuser` — manage superuser accounts
//! - `version`   — print version information

mod cli;

use std::process;
use std::sync::Arc;

use clap::Parser;
use tracing::info;

use zerobase_auth::{Argon2Hasher, JwtTokenService, SmtpEmailService};
use zerobase_core::configuration::StorageBackend;
use zerobase_core::email::templates::EmailTemplateEngine;
use zerobase_core::email::{EmailService, NoopEmailService};
use zerobase_core::oauth::OAuthProviderRegistry;
use zerobase_core::telemetry::{init_tracing, LogFormat};
use zerobase_core::services::settings_service::CorsSettingsDto;
use zerobase_core::{
    BackupService, CollectionService, LogService, RecordService, SettingsService, Settings,
    SuperuserService,
};
use zerobase_db::migrations::{run_migrations, system::system_migrations};
use zerobase_db::{Database, PoolConfig};
use zerobase_files::{FileService, LocalFileStorage, S3FileStorage};

use cli::{Cli, Command, SuperuserAction};

// ── Entry point ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        eprintln!("error: {e:#}");
        process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Some(Command::Serve(args)) => cmd_serve(args).await,
        Some(Command::Migrate(args)) => cmd_migrate(args),
        Some(Command::Superuser(args)) => cmd_superuser(args),
        Some(Command::Version) | None => {
            cmd_version();
            Ok(())
        }
    }
}

// ── serve ───────────────────────────────────────────────────────────────────

async fn cmd_serve(args: cli::ServeArgs) -> anyhow::Result<()> {
    let mut settings = Settings::load()?;

    // CLI flags override config/env.
    if let Some(host) = args.host {
        settings.server.host = host;
    }
    if let Some(port) = args.port {
        settings.server.port = port;
    }
    if let Some(data_dir) = args.data_dir {
        settings.database.path = data_dir.join("data.db");
        settings.storage.local_path = data_dir.join("storage");
    }
    if let Some(ref fmt) = args.log_format {
        settings.server.log_format = match fmt.as_str() {
            "pretty" => LogFormat::Pretty,
            _ => LogFormat::Json,
        };
    }

    init_tracing(settings.server.log_format.clone());

    info!(
        version = env!("CARGO_PKG_VERSION"),
        host = %settings.server.host,
        port = %settings.server.port,
        "starting zerobase"
    );

    // ── Database ────────────────────────────────────────────────────────
    let pool_config = PoolConfig::from(&settings.database);
    let db = Database::open(&settings.database.path, &pool_config)?;
    let migrations = system_migrations();
    db.with_write_conn(|conn| {
        run_migrations(conn, &migrations).map_err(|e| e.into())
    })?;
    info!("database migrations applied");

    let db = Arc::new(db);

    // ── Shared services ─────────────────────────────────────────────────
    let token_service: Arc<dyn zerobase_core::auth::TokenService> =
        Arc::new(JwtTokenService::from_settings(&settings.auth));

    let collection_service = Arc::new(CollectionService::new(db.as_ref().clone()));
    let record_service = Arc::new(RecordService::with_password_hasher(
        db.as_ref().clone(),
        Arc::clone(&collection_service),
        Argon2Hasher,
    ));
    let superuser_service = Arc::new(SuperuserService::new(db.as_ref().clone(), Argon2Hasher));
    let backup_service = Arc::new(BackupService::new(db.as_ref().clone()));
    let log_service = Arc::new(LogService::new(
        Arc::clone(&db),
        settings.logs.retention_days,
    ));

    let email_service: Arc<dyn EmailService> =
        match SmtpEmailService::from_settings(&settings.smtp) {
            Some(smtp) => Arc::new(smtp),
            None => Arc::new(NoopEmailService),
        };

    let template_engine = EmailTemplateEngine::new("Zerobase");
    let app_url = format!(
        "http://{}:{}",
        settings.server.host, settings.server.port
    );

    // ── File storage ────────────────────────────────────────────────────
    let file_storage: Arc<dyn zerobase_core::FileStorage> =
        match settings.storage.backend {
            StorageBackend::Local => {
                Arc::new(LocalFileStorage::new(&settings.storage.local_path).await?)
            }
            StorageBackend::S3 => {
                let s3_settings = settings.storage.s3.as_ref()
                    .expect("S3 settings required when backend = s3");
                Arc::new(S3FileStorage::new(s3_settings)?)
            }
        };
    let file_service = Arc::new(FileService::new(file_storage));

    // ── Settings service ─────────────────────────────────────────────────
    let settings_service = Arc::new(SettingsService::new(db.as_ref().clone()));

    // ── CORS ──────────────────────────────────────────────────────────────
    let cors_settings: CorsSettingsDto = settings_service
        .get("cors")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let cors_layer = zerobase_api::build_cors_layer(&cors_settings);

    // ── OAuth2 ──────────────────────────────────────────────────────────
    let mut oauth_registry = OAuthProviderRegistry::new();
    zerobase_auth::register_default_providers(&mut oauth_registry);
    let oauth_registry = Arc::new(oauth_registry);

    // ── Auth middleware state ────────────────────────────────────────────
    let auth_middleware_state = Arc::new(zerobase_api::AuthMiddlewareState {
        token_service: Arc::clone(&token_service),
        record_repo: Arc::new(db.as_ref().clone()),
        schema_lookup: Arc::clone(&collection_service),
    });

    // ── Build router ────────────────────────────────────────────────────
    let body_limit_config = zerobase_api::BodyLimitConfig::new(
        settings.server.body_limit,
        settings.server.body_limit_upload,
    );
    let app = zerobase_api::api_router_with_auth_full(
        auth_middleware_state,
        zerobase_api::RateLimitConfig::default(),
        cors_layer,
        body_limit_config,
    )
        // Admin (superuser) auth
        .merge(zerobase_api::admin_routes(
            Arc::clone(&superuser_service),
            Arc::clone(&token_service),
        ))
        // Collection CRUD
        .merge(zerobase_api::collection_routes(
            Arc::clone(&collection_service),
        ))
        // Record CRUD (with file upload support)
        .merge(zerobase_api::record_routes_with_files(
            Arc::clone(&record_service),
            Some(Arc::clone(&file_service)),
        ))
        // Batch operations
        .merge(zerobase_api::batch_routes(Arc::clone(&record_service)))
        // Data export (superuser-only)
        .merge(zerobase_api::export_routes(Arc::clone(&record_service)))
        // User auth (email/password + refresh)
        .merge(zerobase_api::auth_routes(
            Arc::clone(&record_service),
            Arc::clone(&token_service),
        ))
        // Email verification
        .merge(zerobase_api::verification_routes(
            Arc::clone(&record_service),
            Arc::clone(&token_service),
            Arc::clone(&email_service),
            template_engine.clone(),
            app_url.clone(),
        ))
        // Password reset
        .merge(zerobase_api::password_reset_routes(
            Arc::clone(&record_service),
            Arc::clone(&token_service),
            Arc::clone(&email_service),
            template_engine.clone(),
            app_url.clone(),
        ))
        // Email change
        .merge(zerobase_api::email_change_routes(
            Arc::clone(&record_service),
            Arc::clone(&token_service),
            Arc::clone(&email_service),
            template_engine.clone(),
            app_url.clone(),
        ))
        // OTP
        .merge(zerobase_api::otp_routes(
            Arc::clone(&record_service),
            Arc::clone(&token_service),
            Arc::clone(&email_service),
            template_engine.clone(),
        ))
        // MFA (TOTP)
        .merge(zerobase_api::mfa_routes(
            Arc::clone(&record_service),
            Arc::clone(&token_service),
        ))
        // OAuth2
        .merge(zerobase_api::oauth2_routes(
            Arc::clone(&record_service),
            Arc::clone(&token_service),
            Arc::new(db.as_ref().clone()),
            Arc::clone(&oauth_registry),
        ))
        // External auth identities
        .merge(zerobase_api::external_auth_routes(
            Arc::new(db.as_ref().clone()),
            Arc::clone(&collection_service),
            Arc::new(db.as_ref().clone()),
        ))
        // File serving
        .merge(zerobase_api::file_routes(
            Arc::clone(&file_service),
            Arc::clone(&token_service),
            Arc::clone(&collection_service),
        ))
        // Backups
        .merge(zerobase_api::backup_routes(Arc::clone(&backup_service)))
        // Logs
        .merge(zerobase_api::log_routes(Arc::clone(&log_service)))
        // Settings
        .merge(zerobase_api::settings_routes(
            Arc::clone(&settings_service),
            Arc::clone(&email_service),
        ))
        // OpenAPI documentation
        .merge(zerobase_api::openapi_routes(Arc::clone(&collection_service)))
        // Admin dashboard (static files)
        .merge(zerobase_admin::dashboard::dashboard_routes());

    // ── Serve ───────────────────────────────────────────────────────────
    let addr = settings.server.address();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(address = %addr, "listening");

    let coordinator = zerobase::shutdown::ShutdownCoordinator::new(Arc::clone(&db))
        .with_timeout(std::time::Duration::from_secs(settings.server.shutdown_timeout_secs));

    zerobase::shutdown::serve_with_shutdown(listener, app, coordinator).await
}

// ── migrate ─────────────────────────────────────────────────────────────────

fn cmd_migrate(args: cli::MigrateArgs) -> anyhow::Result<()> {
    let settings = Settings::load()?;
    init_tracing(LogFormat::Pretty);

    let db_path = match args.data_dir {
        Some(dir) => dir.join("data.db"),
        None => settings.database.path.clone(),
    };

    info!(path = %db_path.display(), "running migrations");

    let pool_config = PoolConfig::from(&settings.database);
    let db = Database::open(&db_path, &pool_config)?;
    let migrations = system_migrations();

    db.with_write_conn(|conn| {
        run_migrations(conn, &migrations).map_err(|e| e.into())
    })?;

    let version = db.with_write_conn(|conn| {
        zerobase_db::migrations::current_version(conn).map_err(|e| e.into())
    })?;

    println!("Migrations complete. Current schema version: {version}");
    Ok(())
}

// ── superuser ───────────────────────────────────────────────────────────────

fn cmd_superuser(args: cli::SuperuserArgs) -> anyhow::Result<()> {
    let settings = Settings::load()?;
    init_tracing(LogFormat::Pretty);

    let pool_config = PoolConfig::from(&settings.database);
    let db = Database::open(&settings.database.path, &pool_config)?;

    // Ensure system tables exist before operating on superusers.
    let migrations = system_migrations();
    db.with_write_conn(|conn| {
        run_migrations(conn, &migrations).map_err(|e| e.into())
    })?;

    let hasher = Argon2Hasher;
    let service = zerobase_core::SuperuserService::new(db.clone(), hasher);

    match args.action {
        SuperuserAction::Create(create_args) => {
            let record = service.create_superuser(&create_args.email, &create_args.password)?;
            let id = record.get("id").and_then(|v| v.as_str()).unwrap_or("-");
            let email = record.get("email").and_then(|v| v.as_str()).unwrap_or("-");
            println!("Superuser created: {email} (id: {id})");
        }
        SuperuserAction::Update(update_args) => {
            let record = service.update_superuser(
                &update_args.email,
                update_args.new_email.as_deref(),
                update_args.new_password.as_deref(),
            )?;
            let email = record.get("email").and_then(|v| v.as_str()).unwrap_or("-");
            println!("Superuser updated: {email}");
        }
        SuperuserAction::Delete(delete_args) => {
            let record = service
                .find_by_email(&delete_args.email)?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "superuser with email '{}' not found",
                        delete_args.email
                    )
                })?;
            let id = record
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("superuser record missing id"))?;
            service.delete_superuser(id)?;
            println!("Superuser deleted: {}", delete_args.email);
        }
        SuperuserAction::List => {
            let list = service.list_superusers()?;
            if list.is_empty() {
                println!("No superusers found.");
            } else {
                println!("{:<20} {:<36}", "EMAIL", "ID");
                println!("{}", "-".repeat(56));
                for record in &list {
                    let email = record.get("email").and_then(|v| v.as_str()).unwrap_or("-");
                    let id = record.get("id").and_then(|v| v.as_str()).unwrap_or("-");
                    println!("{:<20} {:<36}", email, id);
                }
                println!("\nTotal: {}", list.len());
            }
        }
    }

    Ok(())
}

// ── version ─────────────────────────────────────────────────────────────────

fn cmd_version() {
    println!("zerobase v{}", env!("CARGO_PKG_VERSION"));
    println!("  git commit:  {}", env!("ZEROBASE_GIT_HASH"));
    println!("  build date:  {}", env!("ZEROBASE_BUILD_DATE"));
    println!("  target:      {}", env!("ZEROBASE_BUILD_TARGET"));
    println!("  rustc:       {}", env!("ZEROBASE_RUSTC_VERSION"));
    println!("  profile:     {}", env!("ZEROBASE_BUILD_PROFILE"));
}

