use std::process::ExitCode;

use actix_web::{App, HttpServer, Responder, get, web};
use paleo_api::config::database_options_from_env;
use paleo_api::db::{
    MIGRATOR, STARTUP_WAIT, StartupError, collation_version_warning, connect_with_retry,
    verify_migrations,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgPool};

#[get("/")]
async fn root() -> impl Responder {
    "Paleo API · Open Source project"
}

#[get("/species")]
async fn species() -> impl Responder {
    "List of species"
}

#[get("/eras")]
async fn eras() -> impl Responder {
    "List of eras"
}

#[get("/eras/{era_id}/periods")]
async fn periods(era_id: web::Path<String>) -> impl Responder {
    format!("List of periods for era {}", era_id)
}

#[get("/domains")]
async fn domains() -> impl Responder {
    "List of domains"
}

#[get("/domains/{domain_id}/kingdoms")]
async fn domain_kingdoms(domain_id: web::Path<String>) -> impl Responder {
    format!("List of kingdoms for domain {}", domain_id)
}

#[get("/kingdoms/")]
async fn kingdoms() -> impl Responder {
    "List of kingdoms"
}

#[get("/kingdoms/{kingdom_id}/phyla")]
async fn kingdom_phyla(kingdom_id: web::Path<String>) -> impl Responder {
    format!("List of phyla for kingdom {}", kingdom_id)
}

#[get("/phyla")]
async fn phyla() -> impl Responder {
    "List of phyla"
}

#[get("/phyla/{phyla_id}/classes")]
async fn phyla_classes(phyla_id: web::Path<String>) -> impl Responder {
    format!("List of classes for phyla {}", phyla_id)
}

#[get("/classes")]
async fn classes() -> impl Responder {
    "List of classes"
}

#[get("/classes/{class_id}/orders")]
async fn class_orders(class_id: web::Path<String>) -> impl Responder {
    format!("List of orders for class {}", class_id)
}

#[get("/orders")]
async fn orders() -> impl Responder {
    "List of orders"
}

#[get("/orders/{order_id}/families")]
async fn order_families(order_id: web::Path<String>) -> impl Responder {
    format!("List of families for order {}", order_id)
}

#[get("/families")]
async fn families() -> impl Responder {
    "List of families"
}

#[get("/families/{family_id}/genera")]
async fn family_genera(family_id: web::Path<String>) -> impl Responder {
    format!("List of genera for family {}", family_id)
}

#[get("/genera")]
async fn genera() -> impl Responder {
    "List of genera"
}

/// Startup order: config → connect → migration check → collation check → bind
/// (plan §Startup contract). Never applies migrations.
async fn check_database() -> Result<PgConnectOptions, StartupError> {
    let opts = database_options_from_env(|key| std::env::var(key).ok())?;
    let mut conn = connect_with_retry(&opts, STARTUP_WAIT).await?;
    for warning in verify_migrations(&mut conn, &MIGRATOR).await? {
        eprintln!("paleo_api: warning: {warning}");
    }
    if let Some(warning) = collation_version_warning(&mut conn).await {
        eprintln!("paleo_api: warning: {warning}");
    }
    let _ = conn.close().await;
    Ok(opts)
}

#[actix_web::main]
async fn main() -> ExitCode {
    let opts = match check_database().await {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("paleo_api: {err}");
            return ExitCode::FAILURE;
        }
    };
    let pool: PgPool = PgPoolOptions::new()
        .max_connections(10)
        .connect_lazy_with(opts);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .service(root)
            .service(species)
            .service(eras)
            .service(periods)
            .service(
                web::scope("/taxonomy")
                    .service(domains)
                    .service(domain_kingdoms)
                    .service(kingdoms)
                    .service(kingdom_phyla)
                    .service(phyla)
                    .service(phyla_classes)
                    .service(classes)
                    .service(class_orders)
                    .service(orders)
                    .service(order_families)
                    .service(families)
                    .service(family_genera)
                    .service(genera),
            )
    })
    .bind(("127.0.0.1", 8000));

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
