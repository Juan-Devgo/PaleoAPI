//! HTTP API under `/api/v1` (spec 002, plan §Request pipeline).

pub mod auth;
pub mod db_error;
pub mod decimal;
pub mod error;
pub mod geography;
pub mod geologic_time;
pub mod http;
pub mod input;
pub mod params;
pub mod species;
pub mod taxonomy;

use std::sync::Arc;

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceFactory, ServiceRequest, ServiceResponse};
use actix_web::http::Method;
use actix_web::{App, web};
use sqlx::PgPool;

use self::auth::AdminGate;

/// Every path under `/api/v1` and the methods it answers (OpenAPI drift test).
pub const ROUTES: &[(&str, &[Method])] = &[];

/// Builds the application: routes, CORS headers, `OPTIONS`, `404`/`405` envelopes.
pub fn app(
    pool: PgPool,
    gate: Arc<dyn AdminGate>,
) -> App<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse<impl MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .app_data(web::Data::new(pool))
        .app_data(web::Data::from(gate))
}
