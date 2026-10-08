//! The admin identity of a write and the login endpoint (Spec 003 FR-007–FR-009,
//! FR-015, plan §Login, research R16).

use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use super::error::{ApiError, TokenProblem};
use super::http::{One, read_json};
use super::input::{Obj, is_slug};
use crate::db::DB_TIMEOUT;
use crate::security::Security;
use crate::security::accounts::{self, ACTIVE};
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

/// `POST /api/v1/auth/login` (plan §Login steps 2, 3, 6–8).
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
    let body = read_json(&req, payload).await?;
    let (username, password) = login_body(body)?;

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

    let hash = match &account {
        Some(account) => account.password_hash.clone(),
        None => security.dummy_hash().to_string(),
    };
    let verified = password::verify(password, hash)
        .await
        .map_err(|_| ApiError::internal())?;

    match account {
        Some(account) if verified && account.status == ACTIVE => {
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
        _ => Err(ApiError::invalid_credentials()),
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
