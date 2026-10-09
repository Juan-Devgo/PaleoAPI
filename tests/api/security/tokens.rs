//! Token rejection on writes (003 AC 3, AC 4, AC 25; FR-012, FR-013, FR-018, research R19).
//! Forged tokens are built at run time, so no token literal is committed (FR-016).

use actix_web::http::Method;
use actix_web::test::TestRequest;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, get_current_timestamp};
use paleo_api::security::token::{Claims, issue};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

const MISSING_CHALLENGE: &str = r#"Bearer realm="paleo-api""#;
const REJECTED_CHALLENGE: &str = r#"Bearer realm="paleo-api", error="invalid_token""#;
const MISSING: &str = "This operation needs an admin token. Log in at POST /api/v1/auth/login \
                       and send the token as 'Authorization: Bearer <token>'.";
const EXPIRED: &str =
    "Your token has expired. Log in again at POST /api/v1/auth/login to get a new one.";
const INVALID: &str =
    "Your token is not valid. Log in again at POST /api/v1/auth/login to get a new one.";

fn mesozoic() -> Value {
    json!({ "id": "mesozoic", "name": "Mesozoic", "start_mya": 251.902, "end_mya": 66 })
}

/// `POST /api/v1/eras` with `Authorization: <value>` (or none).
async fn write_with(app: &TestApp, authorization: Option<&str>) -> Resp {
    let mut req = TestRequest::post()
        .uri("/api/v1/eras")
        .insert_header(("Content-Type", "application/json"))
        .set_payload(serde_json::to_vec(&mesozoic()).unwrap());
    if let Some(value) = authorization {
        req = req.insert_header(("Authorization", value.to_string()));
    }
    app.call(req).await
}

async fn write_bearer(app: &TestApp, token: &str) -> Resp {
    write_with(app, Some(&format!("Bearer {token}"))).await
}

#[track_caller]
fn assert_401(resp: &Resp, message: &str, challenge: &str, case: &str) {
    assert_eq!(resp.status.as_u16(), 401, "{case}: {}", resp.text());
    let body = resp.json();
    assert_eq!(body["error"]["code"], "UNAUTHORIZED", "{case}");
    assert_eq!(body["error"]["message"], message, "{case}");
    assert_eq!(resp.header("www-authenticate"), Some(challenge), "{case}");
}

async fn no_era_was_created(app: &TestApp) {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM eras")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "a rejected write changed data");
}

fn claims(now: u64) -> Value {
    json!({
        "sub": ADMIN, "iss": "paleo-api", "aud": "paleo-api",
        "iat": now, "nbf": now, "exp": now + 3600, "ver": 1
    })
}

fn without(mut claims: Value, key: &str) -> Value {
    claims.as_object_mut().unwrap().remove(key);
    claims
}

fn with(mut claims: Value, key: &str, value: Value) -> Value {
    claims[key] = value;
    claims
}

fn sign(alg: Algorithm, claims: &Value, secret: &[u8]) -> String {
    encode(&Header::new(alg), claims, &EncodingKey::from_secret(secret)).unwrap()
}

/// `header.payload.signature` from JSON parts and a raw signature.
fn forge(header: &Value, payload: &Value, signature: &[u8]) -> String {
    format!(
        "{}.{}.{}",
        b64url(header.to_string().as_bytes()),
        b64url(payload.to_string().as_bytes()),
        b64url(signature)
    )
}

#[sqlx::test]
async fn rejected_tokens_are_401_invalid(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let secret = TEST_SECRET.as_bytes();
    let now = get_current_timestamp();
    let valid = app.admin_token();
    let parts: Vec<&str> = valid.split('.').collect();
    assert_eq!(parts.len(), 3);

    let altered = format!(
        "{}.{}.{}",
        parts[0],
        b64url(
            with(claims(now), "exp", json!(now + 86_400))
                .to_string()
                .as_bytes()
        ),
        parts[2]
    );
    let hs256 = json!({ "alg": "HS256", "typ": "JWT" });
    let mut cases: Vec<(&str, String)> = vec![
        ("altered payload", altered),
        ("removed signature", format!("{}.{}.", parts[0], parts[1])),
        (
            "signature segment dropped",
            format!("{}.{}", parts[0], parts[1]),
        ),
        ("garbage", "not-a-token".into()),
        ("empty segments", "..".into()),
        ("empty credential", String::new()),
        (
            "other secret",
            sign(
                Algorithm::HS256,
                &claims(now),
                b"another-secret-0123456789abcdefgh",
            ),
        ),
        ("HS384", sign(Algorithm::HS384, &claims(now), secret)),
        ("HS512", sign(Algorithm::HS512, &claims(now), secret)),
        (
            "RS256",
            forge(
                &json!({ "alg": "RS256", "typ": "JWT" }),
                &claims(now),
                &[7u8; 256],
            ),
        ),
        (
            "wrong issuer",
            sign(
                Algorithm::HS256,
                &with(claims(now), "iss", json!("someone-else")),
                secret,
            ),
        ),
        (
            "wrong audience",
            sign(
                Algorithm::HS256,
                &with(claims(now), "aud", json!("someone-else")),
                secret,
            ),
        ),
        (
            "nbf more than 60 s ahead",
            // 62, not 61: a second boundary between signing and checking must not
            // bring it back within the 60 s tolerance.
            sign(
                Algorithm::HS256,
                &with(claims(now), "nbf", json!(now + 62)),
                secret,
            ),
        ),
        (
            "unknown account",
            sign(
                Algorithm::HS256,
                &with(claims(now), "sub", json!("ghost")),
                secret,
            ),
        ),
        (
            "stale credential version",
            sign(
                Algorithm::HS256,
                &with(claims(now), "ver", json!(2)),
                secret,
            ),
        ),
        (
            "over 4 KB",
            sign(
                Algorithm::HS256,
                &with(claims(now), "pad", json!("x".repeat(4096))),
                secret,
            ),
        ),
        (
            "expired and signed with another secret",
            sign(
                Algorithm::HS256,
                &with(claims(now - 7200), "exp", json!(now - 3600)),
                b"another-secret-0123456789abcdefgh",
            ),
        ),
    ];
    for alg in ["none", "NONE", "None"] {
        cases.push((
            "unsigned",
            forge(&json!({ "alg": alg, "typ": "JWT" }), &claims(now), b""),
        ));
    }
    for claim in ["exp", "nbf", "sub", "ver", "iss", "aud", "iat"] {
        cases.push((
            "missing claim",
            sign(Algorithm::HS256, &without(claims(now), claim), secret),
        ));
    }
    // A forged HS256 header with a signature that does not match.
    cases.push(("bad signature", forge(&hs256, &claims(now), &[0u8; 32])));

    for (case, token) in &cases {
        let resp = write_bearer(&app, token).await;
        assert_401(&resp, INVALID, REJECTED_CHALLENGE, case);
    }
    no_era_was_created(&app).await;
}

#[sqlx::test]
async fn expired_tokens_are_401_expired(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let now = get_current_timestamp();
    // exp = now − 61: beyond the 60 s tolerance.
    let expired = issue(app.security.keys(), &Claims::new(ADMIN, 1, now - 3600 - 61)).unwrap();
    let resp = write_bearer(&app, &expired).await;
    assert_401(&resp, EXPIRED, REJECTED_CHALLENGE, "expired");
    // Within the tolerance it still works.
    let recent = issue(app.security.keys(), &Claims::new(ADMIN, 1, now - 3600 + 30)).unwrap();
    let resp = write_bearer(&app, &recent).await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
}

#[sqlx::test]
async fn tokens_outside_the_authorization_header_are_ignored(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let token = app.admin_token();
    let json_req = |uri: String, body: Value| {
        TestRequest::post()
            .uri(&uri)
            .insert_header(("Content-Type", "application/json"))
            .set_payload(serde_json::to_vec(&body).unwrap())
    };
    let mut body = mesozoic();
    body["access_token"] = json!(token);
    let requests = [
        (
            "query",
            json_req(format!("/api/v1/eras?access_token={token}"), mesozoic()),
        ),
        (
            "cookie",
            json_req("/api/v1/eras".into(), mesozoic())
                .insert_header(("Cookie", format!("access_token={token}; token={token}"))),
        ),
        ("body", json_req("/api/v1/eras".into(), body)),
    ];
    for (case, req) in requests {
        let resp = app.call(req).await;
        assert_401(&resp, MISSING, MISSING_CHALLENGE, case);
    }
    // Another scheme is not a Bearer credential: missing.
    for value in [
        format!("Basic {token}"),
        format!("Token {token}"),
        token.clone(),
    ] {
        let resp = write_with(&app, Some(&value)).await;
        assert_401(&resp, MISSING, MISSING_CHALLENGE, &value);
    }
    no_era_was_created(&app).await;
    // The scheme name is case-insensitive (RFC 9110 §11.1).
    let resp = write_with(&app, Some(&format!("bearer {token}"))).await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
}

#[sqlx::test]
async fn three_distinct_401_messages(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let now = get_current_timestamp();
    let expired = issue(
        app.security.keys(),
        &Claims::new(ADMIN, 1, now - 3600 - 120),
    )
    .unwrap();
    let valid = app.admin_token();
    // Change the first signature character: always alters the first signature byte.
    let (signed, signature) = valid.rsplit_once('.').unwrap();
    let first = if signature.starts_with('A') { 'B' } else { 'A' };
    let tampered = format!("{signed}.{first}{}", &signature[1..]);

    let missing = write_with(&app, None).await;
    let expired = write_bearer(&app, &expired).await;
    let invalid = write_bearer(&app, &tampered).await;
    assert_401(&missing, MISSING, MISSING_CHALLENGE, "missing");
    assert_401(&expired, EXPIRED, REJECTED_CHALLENGE, "expired");
    assert_401(&invalid, INVALID, REJECTED_CHALLENGE, "invalid");
    for resp in [&missing, &expired, &invalid] {
        let message = resp.json()["error"]["message"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase();
        assert!(message.contains("log in"), "says how to fix it: {message}");
        for step in [
            "signature",
            "alg",
            "claim",
            "issuer",
            "audience",
            "hs256",
            "jwt",
        ] {
            assert!(!message.contains(step), "names a step ({step}): {message}");
        }
    }

    // The same applies to every write method.
    for (method, uri) in [
        (Method::PATCH, "/api/v1/eras/mesozoic"),
        (Method::DELETE, "/api/v1/eras/mesozoic"),
    ] {
        let resp = send_as(&app, method.clone(), uri, json!({}), None).await;
        assert_401(&resp, MISSING, MISSING_CHALLENGE, uri);
        let resp = send_as(&app, method, uri, json!({}), Some(&tampered)).await;
        assert_401(&resp, INVALID, REJECTED_CHALLENGE, uri);
    }
}
