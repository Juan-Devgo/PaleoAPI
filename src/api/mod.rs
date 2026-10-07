//! HTTP API under `/api/v1` (spec 002, plan §Request pipeline).

/// Declares a fixed read or lookup statement once: a `pub const <NAME>_SQL` with its
/// text (so tests `EXPLAIN` exactly what runs) and a function running it through the
/// compile-time checked `query_as!` (research R3, plan §Project Structure).
macro_rules! fixed_query {
    (@ret fetch_all $row:ty) => { Vec<$row> };
    (@ret fetch_optional $row:ty) => { Option<$row> };
    (@ret fetch_one $row:ty) => { $row };
    (
        $(#[$doc:meta])*
        $vis:vis const $name:ident = $sql:tt;
        $fvis:vis fn $f:ident($($arg:ident: $ty:ty),* $(,)?) -> $mode:ident $row:ty;
    ) => {
        $(#[$doc])*
        $vis const $name: &str = $sql;

        $fvis async fn $f(
            conn: &mut sqlx::PgConnection,
            $($arg: $ty),*
        ) -> sqlx::Result<fixed_query!(@ret $mode $row)> {
            sqlx::query_as!($row, $sql, $($arg),*).$mode(conn).await
        }
    };
}

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
use actix_web::dev::{Service, ServiceFactory, ServiceRequest, ServiceResponse};
use actix_web::http::header::{self, HeaderValue};
use actix_web::http::{Method, StatusCode};
use actix_web::middleware::{DefaultHeaders, ErrorHandlerResponse, ErrorHandlers};
use actix_web::{App, HttpRequest, HttpResponse, ResponseError, web};
use sqlx::PgPool;

use self::auth::AdminGate;
use self::error::{ApiError, no_store_on_get_errors};

/// Every path under `/api/v1` and the methods it answers (OpenAPI drift test).
pub const ROUTES: &[(&str, &[Method])] = &[];

/// Registers every resource under `/api/v1`.
fn routes(_cfg: &mut web::ServiceConfig) {}

/// Builds the application: routes, CORS headers, `OPTIONS`, `404`/`405` envelopes
/// (plan §Request pipeline).
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
        .service(web::scope("/api/v1").configure(routes))
        .default_service(web::to(route_not_found))
        .wrap(ErrorHandlers::new().handler(StatusCode::METHOD_NOT_ALLOWED, method_not_allowed))
        .wrap_fn(|req, srv| {
            let call = if req.method() == Method::OPTIONS {
                Err(req)
            } else {
                Ok(srv.call(req))
            };
            async move {
                match call {
                    Err(req) => Ok(req.into_response(preflight()).map_into_boxed_body()),
                    Ok(fut) => {
                        let mut res = fut.await?;
                        no_store_on_get_errors(&mut res);
                        Ok(res.map_into_boxed_body())
                    }
                }
            }
        })
        .wrap(
            DefaultHeaders::new()
                .add((header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"))
                .add((header::ACCESS_CONTROL_EXPOSE_HEADERS, "ETag")),
        )
}

/// Runs a list: the count first, then the page unless it lies past the end
/// (plan §Read path Execution).
pub(crate) async fn paged<R>(
    conn: &mut sqlx::PgConnection,
    page: params::Page,
    count_sql: impl FnOnce(&mut sqlx::QueryBuilder<sqlx::Postgres>),
    page_sql: impl FnOnce(&mut sqlx::QueryBuilder<sqlx::Postgres>),
) -> sqlx::Result<(i64, Vec<R>)>
where
    R: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
{
    let mut qb = sqlx::QueryBuilder::new("");
    count_sql(&mut qb);
    let total: i64 = qb.build_query_scalar().fetch_one(&mut *conn).await?;
    if page.offset() >= total {
        return Ok((total, Vec::new()));
    }
    let mut qb = sqlx::QueryBuilder::new("");
    page_sql(&mut qb);
    let rows = qb.build_query_as::<R>().fetch_all(&mut *conn).await?;
    Ok((total, rows))
}

/// One snapshot for every statement of a read (research R8).
pub(crate) async fn read_tx(
    pool: &PgPool,
) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    pool.begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .await
}

/// Every `OPTIONS` request is a successful preflight for public reads (research R6).
fn preflight() -> HttpResponse {
    HttpResponse::NoContent()
        .insert_header((header::ACCESS_CONTROL_ALLOW_METHODS, "GET"))
        .insert_header((header::ACCESS_CONTROL_ALLOW_HEADERS, "If-None-Match"))
        .insert_header((header::ACCESS_CONTROL_MAX_AGE, "86400"))
        .finish()
}

async fn route_not_found(req: HttpRequest) -> HttpResponse {
    ApiError::route_not_found(req.path()).error_response()
}

/// Rewrites actix's empty `405` into the `METHOD_NOT_ALLOWED` envelope, keeping `Allow`.
fn method_not_allowed<B>(res: ServiceResponse<B>) -> actix_web::Result<ErrorHandlerResponse<B>> {
    let (req, res) = res.into_parts();
    let allow = res.headers().get(header::ALLOW).cloned();
    let allowed = allow
        .as_ref()
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mut new = ApiError::method_not_allowed(req.method(), &allowed).error_response();
    if let Some(allow) = allow {
        new.headers_mut().insert(header::ALLOW, allow);
    }
    new.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok(ErrorHandlerResponse::Response(
        ServiceResponse::new(req, new).map_into_right_body(),
    ))
}
