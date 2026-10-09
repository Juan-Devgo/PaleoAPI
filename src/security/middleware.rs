//! Security middleware: `security_headers`, `request_limits`, `capacity`, `rate_limit`,
//! `require_admin` (plan §Request pipeline).

use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::Method;
use actix_web::http::header::{self, HeaderMap, HeaderValue};
use actix_web::middleware::Next;
use actix_web::rt::time::timeout;
use actix_web::{Error, HttpMessage, ResponseError, web};
use sqlx::PgPool;

use super::Security;
use super::accounts::{self, ACTIVE, ADMIN};
use super::client_ip;
use super::events::{Event, EventKind};
use super::rate_limit::Decision;
use super::token::{self, Rejection};
use crate::api::ROUTES;
use crate::api::error::{ApiError, Overload, TokenProblem};
use crate::db::DB_TIMEOUT;

/// Prefix of every route in [`ROUTES`].
pub const API_PREFIX: &str = "/api/v1";
/// The only non-safe route that needs no token (FR-015).
pub const LOGIN_PATH: &str = "/api/v1/auth/login";

/// Longest accepted request target, in bytes (FR-028).
pub const MAX_URI_BYTES: usize = 2048;
/// Largest accepted total of request header lines, in bytes (FR-028).
pub const MAX_HEADER_BYTES: usize = 16_384;
/// Bytes counted per header line besides name and value (`": "` and CRLF).
const HEADER_LINE_OVERHEAD: usize = 4;

/// Response headers readable by browser scripts (research R4).
pub const EXPOSED_HEADERS: &str = "ETag, Retry-After, RateLimit, RateLimit-Policy";
/// `RateLimit` (draft-ietf-httpapi-ratelimit-headers-11).
const RATELIMIT: &str = "ratelimit";
/// `RateLimit-Policy` (draft-ietf-httpapi-ratelimit-headers-11).
const RATELIMIT_POLICY: &str = "ratelimit-policy";

/// Stored in the request extensions by [`require_admin`] after a successful check
/// (data-model §2); write handlers require it through `api::auth::admin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminIdentity {
    pub username: String,
}

fn is_safe(method: &Method) -> bool {
    method == Method::GET || method == Method::HEAD || method == Method::OPTIONS
}

/// Whether `path` (as received) matches the [`ROUTES`] pattern `pattern`: `{name}`
/// matches one non-empty segment.
fn matches_pattern(path: &str, pattern: &str) -> bool {
    let mut segments = path.split('/');
    let mut parts = pattern.split('/');
    loop {
        match (segments.next(), parts.next()) {
            (None, None) => return true,
            (Some(seg), Some(part)) => {
                let param = part.starts_with('{') && part.ends_with('}');
                if (param && seg.is_empty()) || (!param && seg != part) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// The methods [`ROUTES`] lists for `path`, if a pattern matches it.
fn route_methods(path: &str) -> Option<&'static [Method]> {
    let rest = path.strip_prefix(API_PREFIX)?;
    ROUTES
        .iter()
        .find(|(pattern, _)| matches_pattern(rest, pattern))
        .map(|(_, methods)| *methods)
}

/// The deny-by-default decision of research R16, made before routing.
///
/// Safe methods and `POST` login pass. A path matching a [`ROUTES`] pattern needs a token
/// for the methods listed there; other methods pass, so the router answers `405`. A path
/// matching no pattern passes only when the router matches no resource either
/// (`match_pattern` is `None`, so the router answers `404`): a registered route missing
/// from [`ROUTES`] still needs a token.
pub fn needs_admin(method: &Method, path: &str, match_pattern: Option<&str>) -> bool {
    if is_safe(method) || (method == Method::POST && path == LOGIN_PATH) {
        return false;
    }
    match route_methods(path) {
        Some(methods) => methods.contains(method),
        None => match_pattern.is_some(),
    }
}

/// The `Bearer` token of `Authorization` (research R19). No header or another scheme is
/// [`TokenProblem::Missing`]; a `Bearer` credential that is empty or not text is
/// [`TokenProblem::Invalid`]. The scheme name is case-insensitive.
fn bearer(headers: &HeaderMap) -> Result<&str, TokenProblem> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(TokenProblem::Missing);
    };
    let bytes = value.as_bytes();
    let scheme = b"bearer";
    let is_bearer = bytes.len() >= scheme.len()
        && bytes[..scheme.len()].eq_ignore_ascii_case(scheme)
        && bytes.get(scheme.len()).is_none_or(|b| *b == b' ');
    if !is_bearer {
        return Err(TokenProblem::Missing);
    }
    let token = std::str::from_utf8(&bytes[scheme.len()..])
        .map_err(|_| TokenProblem::Invalid)?
        .trim_matches(' ');
    if token.is_empty() {
        return Err(TokenProblem::Invalid);
    }
    Ok(token)
}

/// A refused write: the response and the security event it raises (FR-039), if any.
struct Denied {
    error: ApiError,
    /// `auth_rejected` (with its reason) or `forbidden`.
    event: Option<(EventKind, Option<&'static str>)>,
    /// Set only when the token names an existing account (FR-040).
    account: Option<String>,
}

impl From<ApiError> for Box<Denied> {
    /// Failures of the check itself (`503`, `500`) raise no `401`/`403` event.
    fn from(error: ApiError) -> Self {
        Box::new(Denied {
            error,
            event: None,
            account: None,
        })
    }
}

fn unauthorized(problem: TokenProblem, account: Option<String>) -> Box<Denied> {
    let reason = match problem {
        TokenProblem::Missing => "missing",
        TokenProblem::Expired => "expired",
        TokenProblem::Invalid => "invalid",
    };
    Box::new(Denied {
        error: ApiError::unauthorized(problem),
        event: Some((EventKind::AuthRejected, Some(reason))),
        account,
    })
}

/// Token → live account check (FR-013, FR-015, research R7): `401` for a missing or
/// rejected token, an unknown or disabled account, or a changed credential version;
/// `403` without the admin role; `503` when the lookup fails or takes longer than
/// [`DB_TIMEOUT`] (research R13, R14), so a write is never allowed unchecked.
async fn authorize(req: &ServiceRequest) -> Result<AdminIdentity, Box<Denied>> {
    let (Some(security), Some(pool)) = (
        req.app_data::<web::Data<Security>>(),
        req.app_data::<web::Data<PgPool>>(),
    ) else {
        return Err(ApiError::internal().into());
    };
    let token = bearer(req.headers()).map_err(|p| unauthorized(p, None))?;
    let claims = token::verify(security.keys(), token).map_err(|r| {
        unauthorized(
            match r {
                Rejection::Expired => TokenProblem::Expired,
                Rejection::Invalid => TokenProblem::Invalid,
            },
            None,
        )
    })?;
    let account = timeout(
        DB_TIMEOUT,
        accounts::find_for_write_check(pool.get_ref(), &claims.sub),
    )
    .await
    .map_err(|_| ApiError::unavailable())?
    .map_err(ApiError::from)?;
    let Some(account) = account else {
        return Err(unauthorized(TokenProblem::Invalid, None));
    };
    if account.status != ACTIVE || account.credentials_version != claims.ver {
        return Err(unauthorized(TokenProblem::Invalid, Some(claims.sub)));
    }
    if account.role.as_deref() != Some(ADMIN) {
        return Err(Box::new(Denied {
            error: ApiError::forbidden(),
            event: Some((EventKind::Forbidden, None)),
            account: Some(claims.sub),
        }));
    }
    Ok(AdminIdentity {
        username: claims.sub,
    })
}

/// Plan §Request pipeline 0, outermost: on every response the Spec 002 CORS headers, the
/// FR-033 set (`nosniff`, CSP, `Referrer-Policy`, HSTS) with any `Server` header removed,
/// and `Cache-Control: no-store` on every response to a method other than `GET`, `HEAD`, and
/// `OPTIONS` (login and writes, FR-035, research R15). It also logs every `503` that
/// carries an [`Overload`] reason, whichever layer or handler produced it (FR-039).
pub async fn security_headers(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let safe = is_safe(req.method());
    let mut res = next.call(req).await?;
    let overload = res.response().extensions().get::<Overload>().copied();
    if let Some(reason) = overload
        && let Some(security) = res.request().app_data::<web::Data<Security>>()
    {
        security.log_overloaded(res.request(), reason.as_str(), res.status().as_u16());
    }
    let headers = res.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(EXPOSED_HEADERS),
    );
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; frame-ancestors 'none'",
        ),
        (header::REFERRER_POLICY, "no-referrer"),
        (
            header::STRICT_TRANSPORT_SECURITY,
            "max-age=63072000; includeSubDomains",
        ),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    headers.remove(header::SERVER);
    if !safe {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    Ok(res.map_into_boxed_body())
}

/// The FR-028 error for a request line or header block over its limit, URI first.
fn over_limits(req: &ServiceRequest) -> Option<ApiError> {
    let uri = req.uri();
    let target = uri
        .path_and_query()
        .map_or_else(|| uri.to_string().len(), |pq| pq.as_str().len());
    if target > MAX_URI_BYTES {
        return Some(ApiError::uri_too_long());
    }
    let headers: usize = req
        .headers()
        .iter()
        .map(|(name, value)| name.as_str().len() + value.len() + HEADER_LINE_OVERHEAD)
        .sum();
    (headers > MAX_HEADER_BYTES).then(ApiError::headers_too_large)
}

/// Plan §Request pipeline 1: `414` for a request target over 2,048 bytes, then `431`
/// for header lines totalling over 16 KB (FR-028, research R11); the handler never runs.
pub async fn request_limits(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    if let Some(err) = over_limits(&req) {
        return Ok(req.into_response(err.error_response()));
    }
    next.call(req)
        .await
        .map(ServiceResponse::map_into_boxed_body)
}

/// Plan §Request pipeline 2: takes one in-flight permit without waiting and holds it
/// until the response is produced; none free → `503` with `Retry-After: 1`
/// (FR-030, research R12).
pub async fn capacity(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let Some(security) = req.app_data::<web::Data<Security>>().cloned() else {
        return Ok(req.into_response(ApiError::internal().error_response()));
    };
    let Some(_permit) = security.requests().try_acquire() else {
        return Ok(req.into_response(ApiError::busy(Overload::Capacity).error_response()));
    };
    next.call(req)
        .await
        .map(ServiceResponse::map_into_boxed_body)
}

/// Plan §Request pipeline 3: resolves the [`client_ip::ClientKey`] into the request extensions
/// (research R5), counts the request against the general limit, and answers `429` over
/// it; every response from here inward carries `RateLimit` and `RateLimit-Policy`
/// (research R2, R4). A limiter failure denies the request (research R14). Nothing here
/// reads `User-Agent` (FR-027) or touches the database (FR-025).
pub async fn rate_limit(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let Some(security) = req.app_data::<web::Data<Security>>().cloned() else {
        return Ok(req.into_response(ApiError::internal().error_response()));
    };
    let peer = req.peer_addr().map(|addr| addr.ip());
    let key = client_ip::resolve(peer, req.headers(), &security.config().trusted_proxies);
    req.extensions_mut().insert(key);
    let limiter = security.general();
    let decision = match limiter.check(key, security.now()) {
        Ok(decision) => decision,
        Err(err) => return Ok(req.into_response(ApiError::from(err).error_response())),
    };
    let mut res = match decision {
        Decision::Reject {
            retry_after, log, ..
        } => {
            if log {
                let mut event = security.request_event(EventKind::RateLimited, req.request(), 429);
                event.reason = Some("general");
                security.emit(&event);
            }
            let limit = format!(
                "Too many requests: the limit is {} requests per minute (bursts up to {}).",
                limiter.per_minute(),
                limiter.burst()
            );
            req.into_response(ApiError::rate_limited(&limit, retry_after).error_response())
        }
        Decision::Allow { .. } => next.call(req).await?.map_into_boxed_body(),
    };
    let headers = res.headers_mut();
    for (name, value) in [
        (RATELIMIT, decision.header()),
        (RATELIMIT_POLICY, limiter.policy()),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(header::HeaderName::from_static(name), value);
        }
    }
    Ok(res)
}

/// The last non-empty segment of `target` (a path or a `Location` value).
fn last_segment(target: &str) -> &str {
    target
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

/// `admin_write` for a successful write by `account`: `resource` is the last segment of
/// `Location` for a `201`, otherwise of the path (contracts/security-events.md).
fn log_admin_write(security: &Security, res: &ServiceResponse<BoxBody>, path: &str, account: &str) {
    let resource = res
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .filter(|_| res.status().as_u16() == 201)
        .map_or_else(|| last_segment(path), last_segment);
    let mut event: Event =
        security.request_event(EventKind::AdminWrite, res.request(), res.status().as_u16());
    event.account = Some(account.to_string());
    event.path = Some(path.to_string());
    event.resource = Some(resource.to_string());
    security.emit(&event);
}

/// Plan §Request pipeline 4.2: requests that [`needs_admin`] are answered `401` / `403`
/// unless the token and the live account allow the write; the others pass untouched and
/// `Authorization` is never read for them (FR-017). Refusals and successful writes are
/// logged (FR-039).
pub async fn require_admin(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let pattern = req.match_pattern();
    if !needs_admin(req.method(), req.path(), pattern.as_deref()) {
        return next
            .call(req)
            .await
            .map(ServiceResponse::map_into_boxed_body);
    }
    let identity = match authorize(&req).await {
        Ok(identity) => identity,
        Err(denied) => {
            if let (Some((kind, reason)), Some(security)) =
                (denied.event, req.app_data::<web::Data<Security>>())
            {
                let status = denied.error.status().as_u16();
                let mut event = security.request_event(kind, req.request(), status);
                event.reason = reason;
                event.account = denied.account;
                security.emit(&event);
            }
            return Ok(req.into_response(denied.error.error_response()));
        }
    };
    let account = identity.username.clone();
    let path = req.path().to_string();
    let security = req.app_data::<web::Data<Security>>().cloned();
    req.extensions_mut().insert(identity);
    let res = next.call(req).await?.map_into_boxed_body();
    if res.status().is_success()
        && let Some(security) = security
    {
        log_admin_write(&security, &res, &path, &account);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use actix_web::middleware::from_fn;
    use actix_web::test::{self, TestRequest};
    use actix_web::{App, HttpResponse};

    use super::*;

    #[actix_web::test]
    async fn security_headers_add_cors_and_no_store_on_non_safe_methods() {
        let app = test::init_service(
            App::new()
                .default_service(web::to(|| async {
                    HttpResponse::Ok()
                        .insert_header((header::CACHE_CONTROL, "public, max-age=300"))
                        .finish()
                }))
                .wrap(from_fn(security_headers)),
        )
        .await;
        for method in [
            Method::GET,
            Method::HEAD,
            Method::OPTIONS,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::PUT,
        ] {
            let req = TestRequest::default()
                .method(method.clone())
                .uri("/x")
                .to_request();
            let res = test::call_service(&app, req).await;
            let get = |name| res.headers().get(name).and_then(|v| v.to_str().ok());
            assert_eq!(get(header::ACCESS_CONTROL_ALLOW_ORIGIN), Some("*"));
            assert_eq!(
                get(header::ACCESS_CONTROL_EXPOSE_HEADERS),
                Some("ETag, Retry-After, RateLimit, RateLimit-Policy")
            );
            assert!(get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS).is_none());
            assert_eq!(get(header::X_CONTENT_TYPE_OPTIONS), Some("nosniff"));
            assert_eq!(get(header::REFERRER_POLICY), Some("no-referrer"));
            assert!(get(header::SERVER).is_none());
            let expected = if is_safe(&method) {
                "public, max-age=300"
            } else {
                "no-store"
            };
            assert_eq!(get(header::CACHE_CONTROL), Some(expected), "{method}");
        }
    }

    fn limits_of(uri: &str, headers: &[(&str, usize)]) -> Option<(u16, String)> {
        let mut req = TestRequest::get().uri(uri);
        for (name, len) in headers {
            req = req.insert_header((*name, "a".repeat(*len)));
        }
        over_limits(&req.to_srv_request()).map(|e| (e.status().as_u16(), e.code().to_string()))
    }

    #[test]
    fn request_targets_over_2048_bytes_are_414() {
        let prefix = "/api/v1/eras?x=";
        let at_limit = format!("{prefix}{}", "a".repeat(MAX_URI_BYTES - prefix.len()));
        assert_eq!(limits_of(&at_limit, &[]), None);
        assert_eq!(
            limits_of(&format!("{at_limit}a"), &[]),
            Some((414, "URI_TOO_LONG".into()))
        );
    }

    #[test]
    fn header_lines_over_16_kib_are_431() {
        // One line counts name + value + 4.
        let value = MAX_HEADER_BYTES - "x-pad".len() - HEADER_LINE_OVERHEAD;
        assert_eq!(limits_of("/x", &[("x-pad", value)]), None);
        assert_eq!(
            limits_of("/x", &[("x-pad", value + 1)]),
            Some((431, "REQUEST_HEADERS_TOO_LARGE".into()))
        );
        // The total over many lines counts.
        let mut req = TestRequest::get().uri("/x");
        for i in 0..17 {
            req = req.append_header((format!("x-pad-{i:02}"), "a".repeat(1_000)));
        }
        assert_eq!(
            over_limits(&req.to_srv_request()).map(|e| e.status().as_u16()),
            Some(431)
        );
        // The URI is checked first.
        let long = format!("/{}", "a".repeat(MAX_URI_BYTES));
        assert_eq!(
            limits_of(&long, &[("x-pad", MAX_HEADER_BYTES)]).map(|e| e.0),
            Some(414)
        );
    }

    fn auth(value: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_bytes(value).unwrap(),
        );
        headers
    }

    #[test]
    fn safe_methods_and_login_need_no_token() {
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            for path in ["/api/v1/eras", "/api/v1/eras/x", "/nowhere"] {
                assert!(!needs_admin(&method, path, Some("/api/v1/eras")));
            }
        }
        assert!(!needs_admin(&Method::POST, LOGIN_PATH, Some(LOGIN_PATH)));
    }

    #[test]
    fn listed_write_methods_need_a_token() {
        let cases = [
            (Method::POST, "/api/v1/eras"),
            (Method::PATCH, "/api/v1/eras/mesozoic"),
            (Method::DELETE, "/api/v1/eras/mesozoic"),
            (Method::POST, "/api/v1/eras/mesozoic/periods"),
            (Method::PATCH, "/api/v1/taxonomy/genera/tyrannosaurus"),
            (Method::POST, "/api/v1/taxonomy/families/x/genera"),
            (Method::DELETE, "/api/v1/species/trex"),
        ];
        for (method, path) in cases {
            assert!(needs_admin(&method, path, None), "{method} {path}");
        }
    }

    #[test]
    fn unlisted_methods_and_unknown_paths_reach_the_router() {
        // 405 from the router
        assert!(!needs_admin(
            &Method::PUT,
            "/api/v1/eras",
            Some("/api/v1/eras")
        ));
        assert!(!needs_admin(&Method::DELETE, "/api/v1/periods", None));
        assert!(!needs_admin(&Method::DELETE, LOGIN_PATH, Some(LOGIN_PATH)));
        assert!(!needs_admin(
            &Method::POST,
            "/api/v1/continents/x/countries",
            None
        ));
        // 404 from the router
        for path in [
            "/",
            "/api/v1/nowhere",
            "/api/v1/eras/",
            "/api/v2/eras",
            "/api/v1",
        ] {
            assert!(!needs_admin(&Method::POST, path, None), "{path}");
        }
        // An empty segment never matches a parameter.
        assert!(!needs_admin(&Method::DELETE, "/api/v1/eras/", None));
    }

    #[test]
    fn a_registered_route_missing_from_routes_is_denied() {
        assert!(needs_admin(
            &Method::POST,
            "/api/v1/debug/reset",
            Some("/api/v1/debug/reset")
        ));
        assert!(needs_admin(
            &Method::DELETE,
            "/api/v1/%65ras/x",
            Some("/api/v1/eras/{era_id}")
        ));
    }

    #[test]
    fn bearer_credentials() {
        assert_eq!(bearer(&HeaderMap::new()), Err(TokenProblem::Missing));
        assert_eq!(bearer(&auth(b"Basic abc")), Err(TokenProblem::Missing));
        assert_eq!(bearer(&auth(b"Bearerabc")), Err(TokenProblem::Missing));
        assert_eq!(bearer(&auth(b"abc.def.ghi")), Err(TokenProblem::Missing));
        assert_eq!(bearer(&auth(b"Bearer")), Err(TokenProblem::Invalid));
        assert_eq!(bearer(&auth(b"Bearer   ")), Err(TokenProblem::Invalid));
        assert_eq!(
            bearer(&auth(b"Bearer \xff\xfe")),
            Err(TokenProblem::Invalid)
        );
        assert_eq!(bearer(&auth(b"Bearer a.b.c")), Ok("a.b.c"));
        assert_eq!(bearer(&auth(b"bEaReR  a.b.c ")), Ok("a.b.c"));
    }
}
