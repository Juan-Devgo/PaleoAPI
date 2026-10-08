use std::process::ExitCode;
use std::sync::Arc;

use actix_web::HttpServer;
use paleo_api::api::{self, auth::AdminGate, auth::DenyAll};
use paleo_api::config::{SecurityConfig, database_options_from_env, security_config_from_env};
use paleo_api::db::{
    MIGRATOR, STARTUP_WAIT, StartupError, api_connect_options, collation_version_warning,
    connect_with_retry, verify_migrations,
};
use paleo_api::security::Security;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgPool};

/// Startup order: `DATABASE_URL` → security settings → connect → migration check →
/// collation check (001 and 003 plan §Startup contract). Never applies migrations.
async fn check_startup() -> Result<(PgConnectOptions, SecurityConfig), StartupError> {
    let env = |key: &str| std::env::var(key).ok();
    let opts = database_options_from_env(env)?;
    let config = security_config_from_env(env)?;
    let mut conn = connect_with_retry(&opts, STARTUP_WAIT).await?;
    for warning in verify_migrations(&mut conn, &MIGRATOR).await? {
        eprintln!("paleo_api: warning: {warning}");
    }
    if let Some(warning) = collation_version_warning(&mut conn).await {
        eprintln!("paleo_api: warning: {warning}");
    }
    let _ = conn.close().await;
    Ok((opts, config))
}

#[actix_web::main]
async fn main() -> ExitCode {
    let (opts, config) = match check_startup().await {
        Ok(checked) => checked,
        Err(err) => {
            eprintln!("paleo_api: {err}");
            return ExitCode::FAILURE;
        }
    };
    // Built before binding: computing the dummy hash is part of startup (plan §Startup contract).
    let _security = Arc::new(Security::new(config));
    let pool: PgPool = PgPoolOptions::new()
        .max_connections(10)
        .connect_lazy_with(api_connect_options(opts));
    // Writes are denied in every build until the security spec (spec §4.4).
    let gate: Arc<dyn AdminGate> = Arc::new(DenyAll);

    let server =
        HttpServer::new(move || api::app(pool.clone(), gate.clone())).bind(("127.0.0.1", 8000));

    let result = match server {
        Ok(server) => server.run().await,
        Err(err) => Err(err),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("paleo_api: server error: {err}");
            ExitCode::FAILURE
        }
    }
}
