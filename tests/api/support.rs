//! Shared helpers for the HTTP contract tests (plan §Test strategy, research R11).

#![allow(dead_code)]

use std::future::Future;
use std::net::{SocketAddr, TcpListener};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use actix_web::http::header;
use actix_web::http::header::HeaderMap;
use actix_web::http::{Method, StatusCode};
use actix_web::test::{self, TestRequest};
use actix_web::web::Bytes;
use paleo_api::config::{SecurityConfig, security_config_from_env};
use paleo_api::db::api_connect_options;
use paleo_api::security::clock::{Clock, ManualClock, SystemClock};
use paleo_api::security::events::CaptureSink;
use paleo_api::security::token::{Claims, issue};
use paleo_api::security::{Capacities, Security};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

#[path = "../db/support.rs"]
pub mod db_support;

#[allow(unused_imports)]
pub use db_support::{
    Chain, class, continent, country_with_continent, domain, era, exec, family, full_chain, genus,
    genus_chain, kingdom, order, period, phylum, rank, species_with_period, utc_year,
};

// ---------------------------------------------------------------- security (003 plan §Helpers)

/// Signing secret of every test app (003 FR-014: 32 bytes or more, no placeholder).
pub const TEST_SECRET: &str = "api-test-signing-secret-0123456789abcdef";
/// Password of the seeded accounts. Test-only; [`SEED_HASH`] is its Argon2id PHC string.
pub const TEST_PASSWORD: &str = "paleo-test-password-2026";
/// Precomputed so seeding costs no hashing (003 research R8 parameters).
pub const SEED_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$cGFsZW90ZXN0c2FsdDAxNg$FxuCnQsiuG9OtdkGNc8925Tfws1BkTpzXnFdHISWNZs";
/// Seeded account with the `admin` role.
pub const ADMIN: &str = "admin";
/// Seeded account without a role (writes are `403`).
pub const USER: &str = "user";

/// Test limit values: every limit at its maximum, so only tests that lower one hit it.
const TEST_LIMITS: &[(&str, &str)] = &[
    ("RATE_LIMIT_PER_MINUTE", "1000000"),
    ("RATE_LIMIT_BURST", "1000000"),
    ("LOGIN_LIMIT_PER_CLIENT", "10000"),
    ("LOGIN_LIMIT_PER_USERNAME", "10000"),
    ("MAX_CONCURRENT_REQUESTS", "65535"),
    ("MAX_CONCURRENT_PASSWORD_HASHES", "64"),
];

/// Security settings read through `security_config_from_env`: `vars` over the test
/// limits and [`TEST_SECRET`].
pub fn security_env(vars: &[(&str, &str)]) -> SecurityConfig {
    let vars: Vec<(String, String)> = vars
        .iter()
        .chain(TEST_LIMITS)
        .chain(&[("JWT_SECRET", TEST_SECRET)])
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    security_config_from_env(move |key| vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
        .unwrap_or_else(|e| panic!("test security settings: {e}"))
}

/// How the app under test builds its `Security`.
pub struct SecuritySettings {
    pub config: SecurityConfig,
    pub capacities: Capacities,
    /// A [`ManualClock`] (reachable through [`TestApp::clock`]) instead of `SystemClock`.
    pub manual_clock: bool,
    /// Seed the [`ADMIN`] and [`USER`] accounts.
    pub seed: bool,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            config: security_env(&[]),
            capacities: Capacities::default(),
            manual_clock: false,
            seed: true,
        }
    }
}

/// Inserts an active account with [`SEED_HASH`] and `role`.
pub async fn seed_account(pool: &PgPool, username: &str, role: Option<&str>) {
    sqlx::query("INSERT INTO accounts (username, password_hash, role) VALUES ($1, $2, $3)")
        .bind(username)
        .bind(SEED_HASH)
        .bind(role)
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("seed account {username}: {e}"));
}

/// A token for `username` at credential version 1 (a freshly seeded account).
pub fn token_for(security: &Security, username: &str) -> String {
    issue(security.keys(), &Claims::now(username, 1)).expect("test token")
}

pub fn admin_token(security: &Security) -> String {
    token_for(security, ADMIN)
}

pub fn user_token(security: &Security) -> String {
    token_for(security, USER)
}

/// A distinct client address per `n` (in-process requests without one share `0.0.0.0`).
pub fn peer(n: u32) -> SocketAddr {
    let [_, a, b, c] = n.to_be_bytes();
    SocketAddr::from(([10, a, b, c], 40_000))
}

/// Unpadded base64url (RFC 4648 §5), for hand-built forged tokens.
pub fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// A pool that never connects: a lazy pool on a closed local port. Any database
/// access fails, so a status that needs none proves none happened.
pub fn closed_pool() -> PgPool {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("free port")
        .port();
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(1))
        .connect_lazy_with(api_connect_options(
            PgConnectOptions::new()
                .host("127.0.0.1")
                .port(port)
                .username("nobody")
                .database("nowhere"),
        ))
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

/// The app under test, its pool (for seeding and checks), and its security state.
pub struct TestApp {
    call: Box<Call>,
    pub pool: PgPool,
    pub security: Arc<Security>,
    sink: Arc<CaptureSink>,
    clock: Option<Arc<ManualClock>>,
}

impl TestApp {
    pub async fn call(&self, req: TestRequest) -> Resp {
        (self.call)(req, false).await
    }

    /// Sends the body without a declared length (`Transfer-Encoding: chunked`).
    pub async fn call_chunked(&self, req: TestRequest) -> Resp {
        (self.call)(req, true).await
    }

    /// The manual clock (only with `SecuritySettings::manual_clock`).
    #[track_caller]
    pub fn clock(&self) -> &ManualClock {
        self.clock
            .as_deref()
            .expect("build the app with SecuritySettings { manual_clock: true, .. }")
    }

    pub fn admin_token(&self) -> String {
        admin_token(&self.security)
    }

    pub fn user_token(&self) -> String {
        user_token(&self.security)
    }
}

/// Every captured security event line, parsed.
pub fn events(app: &TestApp) -> Vec<Value> {
    app.sink
        .lines()
        .iter()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("event line {l:?}: {e}")))
        .collect()
}

/// The app with the test settings: seeded accounts, capture sink, `SystemClock`.
pub async fn app(pool_opts: PgPoolOptions, connect_opts: PgConnectOptions) -> TestApp {
    app_with(pool_opts, connect_opts, SecuritySettings::default()).await
}

/// Builds the pool with the production connect options and the app with `settings`.
pub async fn app_with(
    pool_opts: PgPoolOptions,
    connect_opts: PgConnectOptions,
    settings: SecuritySettings,
) -> TestApp {
    let pool = pool_opts
        .connect_with(api_connect_options(connect_opts))
        .await
        .expect("test pool");
    app_with_pool(pool, settings).await
}

/// The app over an existing pool (for example [`closed_pool`], with `seed: false`).
pub async fn app_with_pool(pool: PgPool, settings: SecuritySettings) -> TestApp {
    if settings.seed {
        seed_account(&pool, ADMIN, Some("admin")).await;
        seed_account(&pool, USER, None).await;
    }
    let sink = Arc::new(CaptureSink::default());
    let manual = settings.manual_clock.then(|| Arc::new(ManualClock::new()));
    let clock: Arc<dyn Clock> = match &manual {
        Some(m) => m.clone(),
        None => Arc::new(SystemClock),
    };
    let security = Arc::new(Security::with(
        settings.config,
        settings.capacities,
        clock,
        sink.clone(),
    ));
    let svc = test::init_service(paleo_api::api::app(pool.clone(), security.clone())).await;
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
        security,
        sink,
        clock: manual,
    }
}

/// `POST /api/v1/auth/login` from `peer`.
pub async fn login(app: &TestApp, username: &str, password: &str, peer: SocketAddr) -> Resp {
    app.call(
        TestRequest::post()
            .uri("/api/v1/auth/login")
            .peer_addr(peer)
            .insert_header((header::CONTENT_TYPE, "application/json"))
            .set_payload(
                serde_json::to_vec(&json!({ "username": username, "password": password })).unwrap(),
            ),
    )
    .await
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

/// A write as the seeded admin.
pub async fn send(app: &TestApp, method: Method, uri: &str, json: Value) -> Resp {
    send_as(app, method, uri, json, Some(&app.admin_token())).await
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

/// A `DELETE` as the seeded admin.
pub async fn delete(app: &TestApp, uri: &str) -> Resp {
    app.call(TestRequest::delete().uri(uri).insert_header((
        header::AUTHORIZATION,
        format!("Bearer {}", app.admin_token()),
    )))
    .await
}

/// A write as the seeded admin with a raw body and optional content type.
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
        .insert_header((
            header::AUTHORIZATION,
            format!("Bearer {}", app.admin_token()),
        ))
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
