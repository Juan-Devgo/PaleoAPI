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
pub const ROUTES: &[(&str, &[Method])] = &[
    ("/eras", &[Method::GET, Method::POST]),
    (
        "/eras/{era_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    ("/eras/{era_id}/periods", &[Method::GET, Method::POST]),
    ("/periods", &[Method::GET]),
    (
        "/periods/{period_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    ("/taxonomy/domains", &[Method::GET, Method::POST]),
    (
        "/taxonomy/domains/{domain_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    (
        "/taxonomy/domains/{domain_id}/kingdoms",
        &[Method::GET, Method::POST],
    ),
    ("/taxonomy/kingdoms", &[Method::GET]),
    (
        "/taxonomy/kingdoms/{kingdom_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    (
        "/taxonomy/kingdoms/{kingdom_id}/phyla",
        &[Method::GET, Method::POST],
    ),
    ("/taxonomy/phyla", &[Method::GET]),
    (
        "/taxonomy/phyla/{phylum_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    (
        "/taxonomy/phyla/{phylum_id}/classes",
        &[Method::GET, Method::POST],
    ),
    ("/taxonomy/classes", &[Method::GET]),
    (
        "/taxonomy/classes/{class_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    (
        "/taxonomy/classes/{class_id}/orders",
        &[Method::GET, Method::POST],
    ),
    ("/taxonomy/orders", &[Method::GET]),
    (
        "/taxonomy/orders/{order_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    (
        "/taxonomy/orders/{order_id}/families",
        &[Method::GET, Method::POST],
    ),
    ("/taxonomy/families", &[Method::GET]),
    (
        "/taxonomy/families/{family_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    (
        "/taxonomy/families/{family_id}/genera",
        &[Method::GET, Method::POST],
    ),
    ("/taxonomy/genera", &[Method::GET]),
    (
        "/taxonomy/genera/{genus_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    ("/continents", &[Method::GET, Method::POST]),
    (
        "/continents/{continent_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    ("/continents/{continent_id}/countries", &[Method::GET]),
    ("/countries", &[Method::GET, Method::POST]),
    (
        "/countries/{country_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
    ("/species", &[Method::GET, Method::POST]),
    (
        "/species/{species_id}",
        &[Method::GET, Method::PATCH, Method::DELETE],
    ),
];

/// Registers every resource under `/api/v1`. Taxonomy registers only the six direct
/// parent/child nested paths, so other pairings fall through to `404` (AC 6.3.6).
fn routes(cfg: &mut web::ServiceConfig) {
    use self::{geography as geo, geologic_time as time};

    cfg.service(
        web::resource("/eras")
            .route(web::get().to(time::list_eras))
            .route(web::post().to(time::create_era)),
    )
    .service(
        web::resource("/eras/{era_id}")
            .route(web::get().to(time::get_era))
            .route(web::patch().to(time::update_era))
            .route(web::delete().to(time::delete_era)),
    )
    .service(
        web::resource("/eras/{era_id}/periods")
            .route(web::get().to(time::list_era_periods))
            .route(web::post().to(time::create_period)),
    )
    .service(web::resource("/periods").route(web::get().to(time::list_periods)))
    .service(
        web::resource("/periods/{period_id}")
            .route(web::get().to(time::get_period))
            .route(web::patch().to(time::update_period))
            .route(web::delete().to(time::delete_period)),
    );

    for rank in taxonomy::RANKS.iter() {
        let base = format!("/taxonomy/{}", rank.plural);
        let item = format!("{base}/{{{}_id}}", rank.singular);
        let mut list =
            web::resource(base.as_str()).route(web::get().to(
                move |req: HttpRequest, pool: web::Data<PgPool>| taxonomy::list(rank, req, pool),
            ));
        if rank.parent.is_none() {
            list = list.route(web::post().to(taxonomy::create_domain));
        }
        cfg.service(list);
        cfg.service(
            web::resource(item.as_str())
                .route(web::get().to(
                    move |req: HttpRequest, path: web::Path<String>, pool: web::Data<PgPool>| {
                        taxonomy::detail(rank, req, path, pool)
                    },
                ))
                .route(web::patch().to(
                    move |req: HttpRequest,
                          path: web::Path<String>,
                          payload: web::Payload,
                          pool: web::Data<PgPool>,
                          gate: web::Data<dyn AdminGate>| {
                        taxonomy::update(rank, req, path, payload, pool, gate)
                    },
                ))
                .route(web::delete().to(
                    move |req: HttpRequest,
                          path: web::Path<String>,
                          pool: web::Data<PgPool>,
                          gate: web::Data<dyn AdminGate>| {
                        taxonomy::delete(rank, req, path, pool, gate)
                    },
                )),
        );
        if let Some(child) = rank.child_rank() {
            let nested = format!("{item}/{}", child.plural);
            cfg.service(
                web::resource(nested.as_str())
                    .route(web::get().to(
                        move |req: HttpRequest,
                              path: web::Path<String>,
                              pool: web::Data<PgPool>| {
                            taxonomy::list_children(child, req, path, pool)
                        },
                    ))
                    .route(web::post().to(
                        move |req: HttpRequest,
                              path: web::Path<String>,
                              payload: web::Payload,
                              pool: web::Data<PgPool>,
                              gate: web::Data<dyn AdminGate>| {
                            taxonomy::create_child(child, req, path, payload, pool, gate)
                        },
                    )),
            );
        }
    }

    cfg.service(
        web::resource("/continents")
            .route(web::get().to(geo::list_continents))
            .route(web::post().to(geo::create_continent)),
    )
    .service(
        web::resource("/continents/{continent_id}")
            .route(web::get().to(geo::get_continent))
            .route(web::patch().to(geo::update_continent))
            .route(web::delete().to(geo::delete_continent)),
    )
    .service(
        web::resource("/continents/{continent_id}/countries")
            .route(web::get().to(geo::list_continent_countries)),
    )
    .service(
        web::resource("/countries")
            .route(web::get().to(geo::list_countries))
            .route(web::post().to(geo::create_country)),
    )
    .service(
        web::resource("/countries/{country_id}")
            .route(web::get().to(geo::get_country))
            .route(web::patch().to(geo::update_country))
            .route(web::delete().to(geo::delete_country)),
    )
    .service(
        web::resource("/species")
            .route(web::get().to(species::list))
            .route(web::post().to(species::create)),
    )
    .service(
        web::resource("/species/{species_id}")
            .route(web::get().to(species::detail))
            .route(web::patch().to(species::update))
            .route(web::delete().to(species::delete)),
    );
}

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

/// A write transaction (READ COMMITTED, 001 plan §Notes 2).
pub(crate) async fn begin_write(
    pool: &PgPool,
) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    pool.begin_with("BEGIN ISOLATION LEVEL READ COMMITTED")
        .await
}

/// Check-order steps 2–3 of a body write: admin gate, then the body (research R5).
pub(crate) async fn write_body(
    req: &HttpRequest,
    payload: web::Payload,
    gate: &dyn AdminGate,
) -> Result<serde_json::Value, ApiError> {
    gate.check(req)?;
    http::read_json(req, payload).await
}

/// `DELETE` of one resource: gate, path `404` under the row lock (Q-W2), dependents
/// `409` (Q-W10), delete, `204` (spec §4.10).
pub(crate) async fn delete_resource(
    req: &HttpRequest,
    gate: &dyn AdminGate,
    pool: &PgPool,
    res: error::Resource,
    id: &str,
    lock: impl AsyncFnOnce(&mut sqlx::PgConnection) -> sqlx::Result<bool>,
    delete: impl AsyncFnOnce(&mut sqlx::PgConnection) -> sqlx::Result<()>,
) -> Result<HttpResponse, ApiError> {
    gate.check(req)?;
    if !input::is_slug(id) {
        return Err(ApiError::not_found(res, id));
    }
    let mut tx = begin_write(pool).await?;
    if !lock(&mut tx).await? {
        return Err(ApiError::not_found(res, id));
    }
    let deps = db_error::dependents(&mut tx, res, id).await?;
    if !deps.is_empty() {
        return Err(db_error::has_dependents(res, id, &deps));
    }
    if let Err(e) = delete(&mut tx).await {
        if !db_error::is_fk_violation(&e) {
            return Err(db_error::internal(&e));
        }
        // A dependent appeared concurrently; report it like the pre-check would.
        tx.rollback().await?;
        let mut conn = pool.acquire().await?;
        let deps = db_error::dependents(&mut conn, res, id).await?;
        return Err(db_error::has_dependents(res, id, &deps));
    }
    tx.commit().await.map_err(|e| db_error::internal(&e))?;
    Ok(HttpResponse::NoContent().finish())
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
