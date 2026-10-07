//! Shared helpers for the HTTP contract tests (plan §Test strategy, research R11).

#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use actix_web::http::header::HeaderMap;
use actix_web::http::{Method, StatusCode};
use actix_web::test::{self, TestRequest};
use actix_web::web::Bytes;
use actix_web::{HttpRequest, http::header};
use paleo_api::api::auth::AdminGate;
use paleo_api::api::error::ApiError;
use paleo_api::db::api_connect_options;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

#[path = "../db/support.rs"]
pub mod db_support;

pub use crate::tokens::{TEST_ADMIN_TOKEN, TEST_USER_TOKEN};
#[allow(unused_imports)]
pub use db_support::{
    Chain, class, continent, country_with_continent, domain, era, exec, family, full_chain, genus,
    genus_chain, kingdom, order, period, phylum, rank, species_with_period, utc_year,
};

/// Test-only admin gate (research R4): the admin token passes, the user token is
/// `403`, anything else `401`.
pub struct TestGate;

impl AdminGate for TestGate {
    fn check(&self, req: &HttpRequest) -> Result<(), ApiError> {
        let auth = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        match auth.and_then(|a| a.strip_prefix("Bearer ")) {
            Some(TEST_ADMIN_TOKEN) => Ok(()),
            Some(TEST_USER_TOKEN) => Err(ApiError::forbidden()),
            _ => Err(ApiError::unauthorized()),
        }
    }
}

/// A fully read response.
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Resp {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!(
                "body is not JSON ({e}): {:?}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// `(request, stream without Content-Length)`.
type Call = dyn Fn(TestRequest, bool) -> Pin<Box<dyn Future<Output = Resp>>>;

/// The app under test and its pool (for seeding and checks).
pub struct TestApp {
    call: Box<Call>,
    pub pool: PgPool,
}

impl TestApp {
    pub async fn call(&self, req: TestRequest) -> Resp {
        (self.call)(req, false).await
    }

    /// Sends the body without a declared length (`Transfer-Encoding: chunked`).
    pub async fn call_chunked(&self, req: TestRequest) -> Resp {
        (self.call)(req, true).await
    }
}

/// Builds the pool with the production connect options and the app with `TestGate`.
pub async fn app(pool_opts: PgPoolOptions, connect_opts: PgConnectOptions) -> TestApp {
    let pool = pool_opts
        .connect_with(api_connect_options(connect_opts))
        .await
        .expect("test pool");
    let svc = test::init_service(paleo_api::api::app(pool.clone(), Arc::new(TestGate))).await;
    let svc = Rc::new(svc);
    TestApp {
        call: Box::new(move |req: TestRequest, chunked: bool| {
            let svc = Rc::clone(&svc);
            Box::pin(async move {
                let mut req = req.to_request();
                if chunked {
                    req.headers_mut().remove(header::CONTENT_LENGTH);
                    req.headers_mut().insert(
                        header::TRANSFER_ENCODING,
                        header::HeaderValue::from_static("chunked"),
                    );
                }
                let res = test::call_service(&*svc, req).await;
                let status = res.status();
                let headers = res.headers().clone();
                let body = test::read_body(res).await;
                Resp {
                    status,
                    headers,
                    body,
                }
            })
        }),
        pool,
    }
}

pub async fn get(app: &TestApp, uri: &str) -> Resp {
    app.call(TestRequest::get().uri(uri)).await
}

pub async fn get_with(app: &TestApp, uri: &str, headers: &[(&str, &str)]) -> Resp {
    let mut req = TestRequest::get().uri(uri);
    for (k, v) in headers {
        req = req.insert_header((*k, *v));
    }
    app.call(req).await
}

/// A write as the test admin.
pub async fn send(app: &TestApp, method: Method, uri: &str, json: Value) -> Resp {
    send_as(app, method, uri, json, Some(TEST_ADMIN_TOKEN)).await
}

/// A write with `token` (or no `Authorization` header).
pub async fn send_as(
    app: &TestApp,
    method: Method,
    uri: &str,
    json: Value,
    token: Option<&str>,
) -> Resp {
    let mut req = TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .set_payload(serde_json::to_vec(&json).unwrap());
    if let Some(t) = token {
        req = req.insert_header((header::AUTHORIZATION, format!("Bearer {t}")));
    }
    app.call(req).await
}

/// A `DELETE` as the test admin.
pub async fn delete(app: &TestApp, uri: &str) -> Resp {
    app.call(
        TestRequest::delete()
            .uri(uri)
            .insert_header((header::AUTHORIZATION, format!("Bearer {TEST_ADMIN_TOKEN}"))),
    )
    .await
}

/// A write as the test admin with a raw body and optional content type.
pub async fn send_raw(
    app: &TestApp,
    method: Method,
    uri: &str,
    content_type: Option<&str>,
    bytes: Vec<u8>,
) -> Resp {
    let mut req = TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((header::AUTHORIZATION, format!("Bearer {TEST_ADMIN_TOKEN}")))
        .set_payload(bytes);
    if let Some(ct) = content_type {
        req = req.insert_header((header::CONTENT_TYPE, ct));
    }
    app.call(req).await
}

pub fn body_json(resp: &Resp) -> Value {
    resp.json()
}

/// Asserts the status and `error.code` of an error envelope.
#[track_caller]
pub fn assert_error(resp: &Resp, status: u16, code: &str) {
    assert_eq!(
        resp.status.as_u16(),
        status,
        "expected {status} {code}, got {}: {}",
        resp.status,
        resp.text()
    );
    let body = resp.json();
    assert_eq!(body["error"]["code"], code, "body: {body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty()),
        "error message missing: {body}"
    );
}

/// Asserts a `422 VALIDATION_FAILED` whose details name exactly `fields`.
#[track_caller]
pub fn assert_details(resp: &Resp, fields: &[&str]) {
    assert_error(resp, 422, "VALIDATION_FAILED");
    let body = resp.json();
    let mut got: Vec<String> = body["error"]["details"]
        .as_array()
        .unwrap_or_else(|| panic!("no details: {body}"))
        .iter()
        .map(|d| d["field"].as_str().unwrap().to_string())
        .collect();
    got.sort();
    got.dedup();
    let mut want: Vec<String> = fields.iter().map(|f| f.to_string()).collect();
    want.sort();
    want.dedup();
    assert_eq!(got, want, "detail fields differ: {body}");
}

/// The `ETag` header.
#[track_caller]
pub fn etag(resp: &Resp) -> String {
    resp.header("etag")
        .unwrap_or_else(|| panic!("no ETag on {}", resp.status))
        .to_string()
}

// ---------------------------------------------------------------- seeders

/// One era, one period, and one genus chain to hang species on: `(genus, period)`.
pub async fn seed_base(pool: &PgPool, prefix: &str) -> (String, String) {
    let era_id = format!("{prefix}-era");
    let period_id = format!("{prefix}-period");
    era(pool, &era_id, "251.902", "66").await.unwrap();
    period(pool, &period_id, &era_id, "201.4", "145")
        .await
        .unwrap();
    let genus_id = genus_chain(pool, prefix).await;
    (genus_id, period_id)
}

/// Inserts a species with explicit `name`, `scientific_name`, and `discovery_year`.
pub async fn species_row(
    pool: &PgPool,
    id: &str,
    genus_id: &str,
    period_id: &str,
    name: &str,
    scientific_name: &str,
    discovery_year: Option<i32>,
) {
    sqlx::query(
        "WITH s AS ( \
           INSERT INTO species (id, genus_id, name, scientific_name, diet, description, discovery_year) \
           VALUES ($1, $2, $3, $4, 'carnivore', 'A test species.', $5) RETURNING id) \
         INSERT INTO species_periods (species_id, period_id) SELECT id, $6 FROM s",
    )
    .bind(id)
    .bind(genus_id)
    .bind(name)
    .bind(scientific_name)
    .bind(discovery_year)
    .bind(period_id)
    .execute(pool)
    .await
    .unwrap_or_else(|e| panic!("seed species {id}: {e}"));
}

/// `data[*].id` of a list response.
#[track_caller]
pub fn ids(resp: &Resp) -> Vec<String> {
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    resp.json()["data"]
        .as_array()
        .unwrap_or_else(|| panic!("no data list: {}", resp.text()))
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_string())
        .collect()
}

/// The `pagination` object of a list response.
#[track_caller]
pub fn pagination(resp: &Resp) -> Value {
    resp.json()["pagination"].clone()
}

// ---------------------------------------------------------------- planner

/// A bind value for [`explain_sql`].
#[derive(Debug, Clone)]
pub enum Bind {
    Text(String),
    Texts(Vec<String>),
    Num(bigdecimal::BigDecimal),
}

pub fn text(s: &str) -> Bind {
    Bind::Text(s.to_string())
}

pub fn texts(ids: &[&str]) -> Bind {
    Bind::Texts(ids.iter().map(|s| s.to_string()).collect())
}

pub fn num(s: &str) -> Bind {
    Bind::Num(s.parse().unwrap())
}

fn explain_prefix(analyze: bool) -> &'static str {
    if analyze {
        "EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF) "
    } else {
        "EXPLAIN (COSTS OFF) "
    }
}

/// `EXPLAIN` of the statement a list builder produces, with its binds, under
/// `enable_seqscan = off` (plan §Cases `query_plans.rs`).
pub async fn explain_with_binds(
    pool: &PgPool,
    analyze: bool,
    build: impl FnOnce(&mut sqlx::QueryBuilder<sqlx::Postgres>),
) -> String {
    let mut tx = pool.begin().await.unwrap();
    exec(&mut *tx, "SET LOCAL enable_seqscan = off")
        .await
        .unwrap();
    let mut qb = sqlx::QueryBuilder::new(explain_prefix(analyze));
    build(&mut qb);
    let sql = qb.sql().as_str().to_string();
    let lines: Vec<String> = qb
        .build_query_scalar()
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_else(|e| panic!("EXPLAIN failed for {sql}: {e}"));
    tx.rollback().await.unwrap();
    lines.join("\n")
}

/// `EXPLAIN` of a fixed statement (`*_SQL` constant) with binds, under
/// `enable_seqscan = off`.
pub async fn explain_sql(pool: &PgPool, sql: &str, binds: Vec<Bind>) -> String {
    let mut tx = pool.begin().await.unwrap();
    exec(&mut *tx, "SET LOCAL enable_seqscan = off")
        .await
        .unwrap();
    let text = format!("{}{sql}", explain_prefix(false));
    let mut q = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(text));
    for b in binds {
        q = match b {
            Bind::Text(s) => q.bind(s),
            Bind::Texts(v) => q.bind(v),
            Bind::Num(n) => q.bind(n),
        };
    }
    let lines = q
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_else(|e| panic!("EXPLAIN failed for {sql}: {e}"));
    tx.rollback().await.unwrap();
    lines.join("\n")
}
