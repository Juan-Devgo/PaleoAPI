//! Security middleware: `security_headers`, `request_limits`, `capacity`, `rate_limit`,
//! `require_admin` (plan §Request pipeline).

use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::Method;
use actix_web::http::header::{self, HeaderMap, HeaderValue};
use actix_web::middleware::Next;
use actix_web::{Error, HttpMessage, ResponseError, web};
use sqlx::PgPool;

use super::Security;
use super::accounts::{self, ACTIVE, ADMIN};
use super::token::{self, Rejection};
use crate::api::ROUTES;
use crate::api::error::{ApiError, TokenProblem};

/// Prefix of every route in [`ROUTES`].
pub const API_PREFIX: &str = "/api/v1";
/// The only non-safe route that needs no token (FR-015).
pub const LOGIN_PATH: &str = "/api/v1/auth/login";

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

/// Token → live account check (FR-013, FR-015, research R7): `401` for a missing or
/// rejected token, an unknown or disabled account, or a changed credential version;
/// `403` without the admin role.
async fn authorize(req: &ServiceRequest) -> Result<AdminIdentity, ApiError> {
    let (Some(security), Some(pool)) = (
        req.app_data::<web::Data<Security>>(),
        req.app_data::<web::Data<PgPool>>(),
    ) else {
        return Err(ApiError::internal());
    };
    let token = bearer(req.headers()).map_err(ApiError::unauthorized)?;
    let claims = token::verify(security.keys(), token).map_err(|r| {
        ApiError::unauthorized(match r {
            Rejection::Expired => TokenProblem::Expired,
            Rejection::Invalid => TokenProblem::Invalid,
        })
    })?;
    let account = accounts::find_for_write_check(pool.get_ref(), &claims.sub).await?;
    let Some(account) = account else {
        return Err(ApiError::unauthorized(TokenProblem::Invalid));
    };
    if account.status != ACTIVE || account.credentials_version != claims.ver {
        return Err(ApiError::unauthorized(TokenProblem::Invalid));
    }
    if account.role.as_deref() != Some(ADMIN) {
        return Err(ApiError::forbidden());
    }
    Ok(AdminIdentity {
        username: claims.sub,
    })
}

/// Plan §Request pipeline 0, outermost: on every response the Spec 002 CORS headers, and
/// `Cache-Control: no-store` on every response to a method other than `GET`, `HEAD`, and
/// `OPTIONS` (login and writes, FR-035, research R15).
pub async fn security_headers(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let safe = is_safe(req.method());
    let mut res = next.call(req).await?;
    let headers = res.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("ETag"),
    );
    if !safe {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    Ok(res.map_into_boxed_body())
}

/// Plan §Request pipeline 4.2: requests that [`needs_admin`] are answered `401` / `403`
/// unless the token and the live account allow the write; the others pass untouched and
/// `Authorization` is never read for them (FR-017).
pub async fn require_admin(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let pattern = req.match_pattern();
    if needs_admin(req.method(), req.path(), pattern.as_deref()) {
        match authorize(&req).await {
            Ok(identity) => {
                req.extensions_mut().insert(identity);
            }
            Err(err) => return Ok(req.into_response(err.error_response())),
        }
    }
    next.call(req)
        .await
        .map(ServiceResponse::map_into_boxed_body)
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
            assert_eq!(get(header::ACCESS_CONTROL_EXPOSE_HEADERS), Some("ETag"));
            assert!(get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS).is_none());
            let expected = if is_safe(&method) {
                "public, max-age=300"
            } else {
                "no-store"
            };
            assert_eq!(get(header::CACHE_CONTROL), Some(expected), "{method}");
        }
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
