//! The admin identity of a write and the login endpoint (Spec 003 FR-007–FR-009,
//! FR-015, plan §Login, research R16).

use std::time::Duration;

use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use super::error::{ApiError, Overload, TokenProblem};
use super::http::{One, read_json};
use super::input::{Obj, is_slug};
use crate::db::DB_TIMEOUT;
use crate::security::Security;
use crate::security::accounts::{self, ACTIVE};
use crate::security::client_ip::ClientKey;
use crate::security::login_limit::{Admission, LoginLimiter, Reserve};
use crate::security::middleware::AdminIdentity;
use crate::security::password::{self, MAX_CHARS};
use crate::security::token::{self, Claims, LIFETIME_SECS};

/// The identity `require_admin` stored for this request. A write handler reached without
/// that check fails closed with `401` (research R16).
pub fn admin(req: &HttpRequest) -> Result<AdminIdentity, ApiError> {
    req.extensions()
        .get::<AdminIdentity>()
        .cloned()
        .ok_or_else(|| ApiError::unauthorized(TokenProblem::Missing))
}

/// `data` of a successful login (FR-007).
#[derive(Debug, Serialize)]
struct LoginResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
}

/// A non-empty string. Never echoes the value.
fn username_field(field: &str, v: &Value) -> Result<String, String> {
    match v.as_str() {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(format!("{field} must be a non-empty string.")),
    }
}

/// A string of 1–128 characters (FR-009). Never echoes the value.
fn password_field(field: &str, v: &Value) -> Result<String, String> {
    match v.as_str() {
        Some(s) if !s.is_empty() && s.chars().count() <= MAX_CHARS => Ok(s.to_string()),
        _ => Err(format!(
            "{field} must be a non-empty string of at most {MAX_CHARS} characters."
        )),
    }
}

/// Plan §Login step 3: an object with exactly `username` and `password`, else `422`.
fn login_body(body: Value) -> Result<(String, String), ApiError> {
    let mut o = Obj::new(body)?;
    let username = o.required("username", username_field);
    let password = o.required("password", password_field);
    o.finish()?;
    match (username, password) {
        (Some(username), Some(password)) => Ok((username, password)),
        _ => Err(ApiError::internal()),
    }
}

/// The window as the 429 messages state it: `15 minutes`, `1 second`, `90 seconds`.
fn window_text(window: Duration) -> String {
    let secs = window.as_secs();
    let (n, unit) = if secs >= 60 && secs.is_multiple_of(60) {
        (secs / 60, "minute")
    } else {
        (secs, "second")
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// `429` of the per-client login log (plan §Login step 1).
fn client_limited(limiter: &LoginLimiter, retry_after: u64) -> ApiError {
    let limit = format!(
        "Too many login attempts from your address: the limit is {} per {}.",
        limiter.per_client(),
        window_text(limiter.window())
    );
    ApiError::rate_limited(&limit, retry_after)
}

/// `429` of the per-username failure log (plan §Login step 4).
fn username_limited(limiter: &LoginLimiter, retry_after: u64) -> ApiError {
    let limit = format!(
        "Too many failed logins for this username: the limit is {} per {}.",
        limiter.per_username(),
        window_text(limiter.window())
    );
    ApiError::rate_limited(&limit, retry_after)
}

/// `POST /api/v1/auth/login` (plan §Login steps 1–8).
///
/// Every failure is the same `401 INVALID_CREDENTIALS` after one Argon2 verification:
/// against the stored hash, or against the dummy hash for an unknown username. A username
/// that is not a slug is unknown (never `422`) and is not looked up.
pub async fn login(
    req: HttpRequest,
    payload: web::Payload,
    pool: web::Data<PgPool>,
    security: web::Data<Security>,
) -> Result<HttpResponse, ApiError> {
    let limiter = security.login();
    let client = req
        .extensions()
        .get::<ClientKey>()
        .copied()
        .unwrap_or(ClientKey::UNKNOWN);

    // 1. Per-client log: full → 429, else record.
    if let Admission::Rejected(rejected) = limiter.admit_client(client, security.now())? {
        return Err(client_limited(limiter, rejected.retry_after));
    }

    // 2–3. Body and shape, before any hashing or database access.
    let body = read_json(&req, payload).await?;
    let (username, password) = login_body(body)?;

    // 4. Per-username reservation. Dropped on every path except a failure.
    let reservation = match limiter.reserve(client, &username, security.now())? {
        Reserve::Reserved(reservation) => reservation,
        Reserve::Rejected(rejected) => {
            return Err(username_limited(limiter, rejected.retry_after));
        }
    };

    // 5. Hashing permit: none → 503 and the reservation is released.
    let _permit = security
        .hashes()
        .try_acquire()
        .ok_or_else(|| ApiError::busy(Overload::Hashing))?;

    // 6. Account lookup under the database timeout.
    let account = if is_slug(&username) {
        actix_web::rt::time::timeout(
            DB_TIMEOUT,
            accounts::find_for_login(pool.get_ref(), &username),
        )
        .await
        .map_err(|_| ApiError::unavailable())??
    } else {
        None
    };

    // 7. Verify against the stored hash, or the dummy hash for an unknown username.
    let hash = match &account {
        Some(account) => account.password_hash.clone(),
        None => security.dummy_hash().to_string(),
    };
    let verified = password::verify(password, hash)
        .await
        .map_err(|_| ApiError::internal())?;

    // 8. Success drops the reservation; a failure is recorded.
    match account {
        Some(account) if verified && account.status == ACTIVE => {
            drop(reservation);
            let claims = Claims::now(&username, account.credentials_version);
            let access_token =
                token::issue(security.keys(), &claims).map_err(|_| ApiError::internal())?;
            Ok(HttpResponse::Ok().json(One {
                data: LoginResponse {
                    access_token,
                    token_type: "Bearer",
                    expires_in: LIFETIME_SECS,
                },
            }))
        }
        _ => {
            reservation.fail(security.now());
            Err(ApiError::invalid_credentials())
        }
    }
}

#[cfg(test)]
mod tests {
    use actix_web::test::TestRequest;
    use serde_json::json;

    use super::*;

    #[test]
    fn admin_needs_the_stored_identity() {
        let req = TestRequest::post().to_http_request();
        let err = admin(&req).unwrap_err();
        assert_eq!(err.status().as_u16(), 401);
        assert_eq!(err.code(), "UNAUTHORIZED");

        let req = TestRequest::post().to_http_request();
        let identity = AdminIdentity {
            username: "alice".into(),
        };
        req.extensions_mut().insert(identity.clone());
        assert_eq!(admin(&req).unwrap(), identity);
    }

    #[test]
    fn window_text_reads_naturally() {
        assert_eq!(window_text(Duration::from_secs(900)), "15 minutes");
        assert_eq!(window_text(Duration::from_secs(60)), "1 minute");
        assert_eq!(window_text(Duration::from_secs(90)), "90 seconds");
        assert_eq!(window_text(Duration::from_secs(1)), "1 second");
    }

    fn fields(err: ApiError) -> Vec<String> {
        assert_eq!(err.code(), "VALIDATION_FAILED");
        err.details().iter().map(|d| d.field.clone()).collect()
    }

    #[test]
    fn login_body_rules() {
        let ok = login_body(json!({ "username": "alice", "password": "x" })).unwrap();
        assert_eq!(ok, ("alice".to_string(), "x".to_string()));
        let max = "é".repeat(MAX_CHARS);
        assert!(login_body(json!({ "username": "Not A Slug", "password": max })).is_ok());

        let long = "p".repeat(MAX_CHARS + 1);
        let err = login_body(json!({ "username": "alice", "password": long })).unwrap_err();
        assert!(!err.message().contains(&long) && !err.details()[0].message.contains(&long));
        assert_eq!(fields(err), ["password"]);
        let err = login_body(json!({ "username": "", "password": "" })).unwrap_err();
        assert_eq!(fields(err), ["username", "password"]);
        let err = login_body(json!({ "username": "a", "password": "b", "extra": 1 })).unwrap_err();
        assert_eq!(fields(err), ["extra"]);
        let err = login_body(json!([1])).unwrap_err();
        assert_eq!(fields(err), [""]);
    }
}
