//! Error envelope, codes (spec §4.12), and field details (spec §4.5).

use std::borrow::Cow;
use std::fmt;

use actix_web::dev::ServiceResponse;
use actix_web::http::header::{self, HeaderValue};
use actix_web::http::{Method, StatusCode};
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;

use super::input::truncate;

/// One problem in a write body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// The resources of spec §5, for codes (`ERA_NOT_FOUND`) and messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Era,
    Period,
    Domain,
    Kingdom,
    Phylum,
    Class,
    Order,
    Family,
    Genus,
    Continent,
    Country,
    Species,
}

impl Resource {
    /// Code stem (data-model §4).
    pub fn code(self) -> &'static str {
        match self {
            Self::Era => "ERA",
            Self::Period => "PERIOD",
            Self::Domain => "DOMAIN",
            Self::Kingdom => "KINGDOM",
            Self::Phylum => "PHYLUM",
            Self::Class => "CLASS",
            Self::Order => "ORDER",
            Self::Family => "FAMILY",
            Self::Genus => "GENUS",
            Self::Continent => "CONTINENT",
            Self::Country => "COUNTRY",
            Self::Species => "SPECIES",
        }
    }

    /// Singular noun for messages.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Era => "era",
            Self::Period => "period",
            Self::Domain => "domain",
            Self::Kingdom => "kingdom",
            Self::Phylum => "phylum",
            Self::Class => "class",
            Self::Order => "order",
            Self::Family => "family",
            Self::Genus => "genus",
            Self::Continent => "continent",
            Self::Country => "country",
            Self::Species => "species",
        }
    }

    /// Plural noun for messages.
    pub fn plural(self) -> &'static str {
        match self {
            Self::Era => "eras",
            Self::Period => "periods",
            Self::Domain => "domains",
            Self::Kingdom => "kingdoms",
            Self::Phylum => "phyla",
            Self::Class => "classes",
            Self::Order => "orders",
            Self::Family => "families",
            Self::Genus => "genera",
            Self::Continent => "continents",
            Self::Country => "countries",
            Self::Species => "species",
        }
    }

    /// "a" or "an" before [`Resource::noun`].
    pub fn article(self) -> &'static str {
        match self {
            Self::Era | Self::Order => "an",
            _ => "a",
        }
    }
}

/// Why a write's token was refused (Spec 003 FR-018, research R19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenProblem {
    /// No `Authorization` header, or a scheme other than `Bearer`.
    Missing,
    /// A `Bearer` token whose signature verified but whose `exp` has passed.
    Expired,
    /// Any other `Bearer` credential that fails (FR-013).
    Invalid,
}

/// `WWW-Authenticate` when no credentials were sent, and on login `401` (research R19).
pub const CHALLENGE: &str = r#"Bearer realm="paleo-api""#;
/// `WWW-Authenticate` when a `Bearer` credential was sent and refused (RFC 6750 §3.1).
pub const CHALLENGE_INVALID_TOKEN: &str = r#"Bearer realm="paleo-api", error="invalid_token""#;

/// An error response: status, code, message, `422` details, and the `WWW-Authenticate`
/// and `Retry-After` headers.
#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: Cow<'static, str>,
    message: String,
    details: Vec<FieldError>,
    challenge: Option<&'static str>,
    retry_after: Option<u64>,
    overload: Option<Overload>,
}

/// The cause of a `503`, as the security log reports it in `overloaded` (FR-039). Stored in
/// the extensions of the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overload {
    /// No free in-flight request permit.
    Capacity,
    /// No free password verification permit.
    Hashing,
    /// The database is unreachable or too slow.
    Database,
    /// A limiter table is full of live keys.
    Limiter,
}

impl Overload {
    /// The `reason` of the event (contracts/security-events.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::Hashing => "hashing",
            Self::Database => "database",
            Self::Limiter => "limiter",
        }
    }
}

/// Longest path id echoed in a message (plan §Write path).
const ID_ECHO: usize = 64;

impl ApiError {
    fn new(status: StatusCode, code: impl Into<Cow<'static, str>>, message: String) -> Self {
        Self {
            status,
            code: code.into(),
            message,
            details: Vec::new(),
            challenge: None,
            retry_after: None,
            overload: None,
        }
    }

    pub fn invalid_json(message: String) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "INVALID_JSON", message)
    }

    pub fn invalid_query(message: String) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "INVALID_QUERY_PARAMETER", message)
    }

    /// `401 UNAUTHORIZED`: the message says what is wrong with the token and how to fix
    /// it, never which check failed (FR-018).
    pub fn unauthorized(problem: TokenProblem) -> Self {
        let (message, challenge) = match problem {
            TokenProblem::Missing => (
                "This operation needs an admin token. Log in at POST /api/v1/auth/login \
                 and send the token as 'Authorization: Bearer <token>'.",
                CHALLENGE,
            ),
            TokenProblem::Expired => (
                "Your token has expired. Log in again at POST /api/v1/auth/login \
                 to get a new one.",
                CHALLENGE_INVALID_TOKEN,
            ),
            TokenProblem::Invalid => (
                "Your token is not valid. Log in again at POST /api/v1/auth/login \
                 to get a new one.",
                CHALLENGE_INVALID_TOKEN,
            ),
        };
        Self {
            challenge: Some(challenge),
            ..Self::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message.into())
        }
    }

    /// Login `401`: identical for an unknown username, a wrong password, and a disabled
    /// account (FR-008).
    pub fn invalid_credentials() -> Self {
        Self {
            challenge: Some(CHALLENGE),
            ..Self::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_CREDENTIALS",
                "Wrong username or password.".into(),
            )
        }
    }

    /// `403`: a valid token of an account without the admin role. No challenge (R19).
    pub fn forbidden() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "This operation requires the admin role. \
             Ask an operator to grant it to your account."
                .into(),
        )
    }

    /// `429 RATE_LIMITED` with `Retry-After` (FR-023): `limit` states the limit, the
    /// message ends with when to retry.
    pub fn rate_limited(limit: &str, retry_after: u64) -> Self {
        let unit = if retry_after == 1 {
            "second"
        } else {
            "seconds"
        };
        Self {
            retry_after: Some(retry_after),
            ..Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                format!("{limit} Retry in {retry_after} {unit}."),
            )
        }
    }

    /// `503` under load, `Retry-After: 1` (research R14): `Capacity`, `Hashing`, or `Limiter`.
    pub fn busy(reason: Overload) -> Self {
        Self {
            retry_after: Some(1),
            overload: Some(reason),
            ..Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "SERVICE_UNAVAILABLE",
                "The API is busy. Retry in 1 second.".into(),
            )
        }
    }

    /// `503` when the database is unavailable or too slow, `Retry-After: 5` (research R14).
    pub fn unavailable() -> Self {
        Self {
            retry_after: Some(5),
            overload: Some(Overload::Database),
            ..Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "SERVICE_UNAVAILABLE",
                "The service is temporarily unavailable. Retry in 5 seconds.".into(),
            )
        }
    }

    /// `404 <R>_NOT_FOUND` naming the id.
    pub fn not_found(res: Resource, id: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            format!("{}_NOT_FOUND", res.code()),
            format!(
                "No {} found with id '{}'.",
                res.noun(),
                truncate(id, ID_ECHO)
            ),
        )
    }

    pub fn route_not_found(path: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "ROUTE_NOT_FOUND",
            format!(
                "No route matches '{}'. See the API description for available paths.",
                truncate(path, 256)
            ),
        )
    }

    /// `405` naming the method and the allowed ones.
    pub fn method_not_allowed(method: &Method, allow: &str) -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "METHOD_NOT_ALLOWED",
            format!(
                "{} is not supported on this path. Use one of: {allow}.",
                truncate(method.as_str(), 16)
            ),
        )
    }

    pub fn already_exists(res: Resource, message: String) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            format!("{}_ALREADY_EXISTS", res.code()),
            message,
        )
    }

    pub fn has_dependents(res: Resource, message: String) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            format!("{}_HAS_DEPENDENTS", res.code()),
            message,
        )
    }

    pub fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PAYLOAD_TOO_LARGE",
            "The request body is larger than 64 KB.".into(),
        )
    }

    /// `408`: the body did not arrive within the deadline (FR-029).
    pub fn request_timeout() -> Self {
        Self::new(
            StatusCode::REQUEST_TIMEOUT,
            "REQUEST_TIMEOUT",
            "The request body did not arrive within 30 seconds. Send the whole body at once."
                .into(),
        )
    }

    /// `414`: the request target is longer than 2,048 bytes (FR-028).
    pub fn uri_too_long() -> Self {
        Self::new(
            StatusCode::URI_TOO_LONG,
            "URI_TOO_LONG",
            "The URL is longer than 2,048 characters. Use shorter query values.".into(),
        )
    }

    /// `431`: the request headers total more than 16 KB (FR-028).
    pub fn headers_too_large() -> Self {
        Self::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "REQUEST_HEADERS_TOO_LARGE",
            "The request headers total more than 16 KB. Send fewer or smaller headers.".into(),
        )
    }

    pub fn unsupported_media_type() -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
            "Send the body as JSON with the header Content-Type: application/json.".into(),
        )
    }

    /// `422 VALIDATION_FAILED` with every detail.
    pub fn validation(details: Vec<FieldError>) -> Self {
        let n = details.len();
        let message = if n == 1 {
            "The request body has 1 invalid field.".to_string()
        } else {
            format!("The request body has {n} invalid fields.")
        };
        Self {
            details,
            ..Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_FAILED",
                message,
            )
        }
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Something went wrong on our side. Try again later.".into(),
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn details(&self) -> &[FieldError] {
        &self.details
    }
    /// The `WWW-Authenticate` value, on `401` only.
    pub fn challenge(&self) -> Option<&'static str> {
        self.challenge
    }
    /// `Retry-After` in seconds.
    pub fn retry_after(&self) -> Option<u64> {
        self.retry_after
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.message)
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    error: Body<'a>,
}

#[derive(Serialize)]
struct Body<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a [FieldError]>,
}

impl ApiError {
    /// The JSON envelope of spec §4.5.
    pub fn body(&self) -> Vec<u8> {
        let details =
            (self.status == StatusCode::UNPROCESSABLE_ENTITY).then_some(&self.details[..]);
        serde_json::to_vec(&Envelope {
            error: Body {
                code: &self.code,
                message: &self.message,
                details,
            },
        })
        .expect("error envelope serializes")
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse {
        let mut res = HttpResponse::build(self.status);
        res.content_type("application/json");
        if let Some(challenge) = self.challenge {
            res.insert_header((header::WWW_AUTHENTICATE, challenge));
        }
        if let Some(secs) = self.retry_after {
            res.insert_header((header::RETRY_AFTER, secs));
        }
        let mut res = res.body(self.body());
        if let Some(reason) = self.overload {
            res.extensions_mut().insert(reason);
        }
        res
    }
}

/// `GET` error responses must not be cached (research R7).
pub fn no_store_on_get_errors<B>(res: &mut ServiceResponse<B>) {
    let method = res.request().method();
    if (method == Method::GET || method == Method::HEAD)
        && (res.status().is_client_error() || res.status().is_server_error())
    {
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes() {
        let e = ApiError::not_found(Resource::Species, "trex01");
        assert_eq!(e.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = serde_json::from_slice(&e.body()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "error": { "code": "SPECIES_NOT_FOUND", "message": "No species found with id 'trex01'." } })
        );
        let e = ApiError::validation(vec![FieldError::new("id", "bad")]);
        let v: serde_json::Value = serde_json::from_slice(&e.body()).unwrap();
        assert_eq!(v["error"]["details"][0]["field"], "id");
        assert_eq!(e.code(), "VALIDATION_FAILED");
    }

    fn header(res: &HttpResponse, name: header::HeaderName) -> Option<&str> {
        res.headers().get(name).and_then(|v| v.to_str().ok())
    }

    #[test]
    fn unauthorized_messages_and_challenges_differ() {
        let cases = [
            (TokenProblem::Missing, CHALLENGE),
            (TokenProblem::Expired, CHALLENGE_INVALID_TOKEN),
            (TokenProblem::Invalid, CHALLENGE_INVALID_TOKEN),
        ];
        let mut messages = Vec::new();
        for (problem, challenge) in cases {
            let e = ApiError::unauthorized(problem);
            assert_eq!(e.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(e.code(), "UNAUTHORIZED");
            assert!(e.message().contains("POST /api/v1/auth/login"));
            let res = e.error_response();
            assert_eq!(header(&res, header::WWW_AUTHENTICATE), Some(challenge));
            assert_eq!(header(&res, header::RETRY_AFTER), None);
            messages.push(e.message().to_string());
        }
        messages.dedup();
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn login_and_role_errors() {
        let e = ApiError::invalid_credentials();
        assert_eq!(e.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(e.code(), "INVALID_CREDENTIALS");
        assert_eq!(e.message(), "Wrong username or password.");
        let res = e.error_response();
        assert_eq!(header(&res, header::WWW_AUTHENTICATE), Some(CHALLENGE));

        let e = ApiError::forbidden();
        assert_eq!(e.status(), StatusCode::FORBIDDEN);
        assert_eq!(header(&e.error_response(), header::WWW_AUTHENTICATE), None);
    }

    #[test]
    fn service_unavailable_carries_retry_after() {
        for (e, secs, reason) in [
            (ApiError::busy(Overload::Capacity), "1", Overload::Capacity),
            (ApiError::busy(Overload::Hashing), "1", Overload::Hashing),
            (ApiError::unavailable(), "5", Overload::Database),
        ] {
            assert_eq!(e.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(e.code(), "SERVICE_UNAVAILABLE");
            assert!(e.message().contains(&format!("Retry in {secs} second")));
            let res = e.error_response();
            assert_eq!(header(&res, header::RETRY_AFTER), Some(secs));
            assert_eq!(header(&res, header::WWW_AUTHENTICATE), None);
            assert_eq!(res.extensions().get::<Overload>(), Some(&reason));
            let v: serde_json::Value = serde_json::from_slice(&e.body()).unwrap();
            assert_eq!(v["error"]["code"], "SERVICE_UNAVAILABLE");
        }
    }

    #[test]
    fn limit_errors_have_the_spec_codes() {
        for (e, status, code) in [
            (ApiError::request_timeout(), 408, "REQUEST_TIMEOUT"),
            (ApiError::uri_too_long(), 414, "URI_TOO_LONG"),
            (
                ApiError::headers_too_large(),
                431,
                "REQUEST_HEADERS_TOO_LARGE",
            ),
        ] {
            assert_eq!(e.status().as_u16(), status);
            assert_eq!(e.code(), code);
            assert!(!e.message().is_empty());
            let res = e.error_response();
            assert_eq!(header(&res, header::RETRY_AFTER), None);
            let v: serde_json::Value = serde_json::from_slice(&e.body()).unwrap();
            assert_eq!(v["error"]["code"], code);
        }
    }
}
