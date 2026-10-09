//! Write authorization (003 AC 2, AC 5, AC 8; FR-005, FR-015, FR-017, FR-035).

use actix_web::http::Method;
use actix_web::test::TestRequest;
use jsonwebtoken::get_current_timestamp;
use paleo_api::security::token::{Claims, issue};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

const MISSING_CHALLENGE: &str = r#"Bearer realm="paleo-api""#;
const FORBIDDEN: &str =
    "This operation requires the admin role. Ask an operator to grant it to your account.";

/// An era body; eras may not overlap, so each id gets its own range.
fn era(id: &str) -> Value {
    let (start, end) = match id {
        "era-a" => (500, 450),
        "era-b" => (440, 400),
        "era-c" => (390, 350),
        "era-d" => (340, 300),
        _ => (250, 66),
    };
    json!({ "id": id, "name": id, "start_mya": start, "end_mya": end })
}

async fn create_era(app: &TestApp, id: &str, token: Option<&str>) -> Resp {
    send_as(app, Method::POST, "/api/v1/eras", era(id), token).await
}

async fn sql(app: &TestApp, statement: &str) {
    exec(&app.pool, statement).await.unwrap();
}

#[sqlx::test]
async fn admin_writes_succeed_with_no_store(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let token = app.admin_token();
    let created = create_era(&app, "mesozoic", Some(&token)).await;
    assert_eq!(created.status.as_u16(), 201, "{}", created.text());
    let updated = send_as(
        &app,
        Method::PATCH,
        "/api/v1/eras/mesozoic",
        json!({ "name": "Mesozoic" }),
        Some(&token),
    )
    .await;
    assert_eq!(updated.status.as_u16(), 200, "{}", updated.text());
    let deleted = app
        .call(
            TestRequest::delete()
                .uri("/api/v1/eras/mesozoic")
                .insert_header(("Authorization", format!("Bearer {token}"))),
        )
        .await;
    assert_eq!(deleted.status.as_u16(), 204, "{}", deleted.text());
    for resp in [&created, &updated, &deleted] {
        assert_eq!(resp.header("cache-control"), Some("no-store"));
        assert_eq!(resp.header("www-authenticate"), None);
    }
}

#[sqlx::test]
async fn missing_token_is_401_and_roleless_is_403(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let resp = create_era(&app, "mesozoic", None).await;
    assert_error(&resp, 401, "UNAUTHORIZED");
    assert_eq!(resp.header("www-authenticate"), Some(MISSING_CHALLENGE));
    assert_eq!(resp.header("cache-control"), Some("no-store"));

    let resp = create_era(&app, "mesozoic", Some(&app.user_token())).await;
    assert_error(&resp, 403, "FORBIDDEN");
    assert_eq!(resp.json()["error"]["message"], FORBIDDEN);
    assert_eq!(
        resp.header("www-authenticate"),
        None,
        "403 has no challenge"
    );
    assert_eq!(resp.header("cache-control"), Some("no-store"));

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM eras")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn credential_changes_take_effect_on_the_next_write(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let token = app.admin_token();
    assert_eq!(create_era(&app, "era-a", Some(&token)).await.status, 201);

    // Role removed → 403; re-granted → allowed again with the same token (research R7).
    sql(
        &app,
        "UPDATE accounts SET role = NULL WHERE username = 'admin'",
    )
    .await;
    assert_error(
        &create_era(&app, "era-b", Some(&token)).await,
        403,
        "FORBIDDEN",
    );
    sql(
        &app,
        "UPDATE accounts SET role = 'admin' WHERE username = 'admin'",
    )
    .await;
    assert_eq!(create_era(&app, "era-b", Some(&token)).await.status, 201);

    // Disabled → 401; enabling again does not revive the earlier token.
    sql(
        &app,
        "UPDATE accounts SET status = 'disabled' WHERE username = 'admin'",
    )
    .await;
    assert_error(
        &create_era(&app, "era-c", Some(&token)).await,
        401,
        "UNAUTHORIZED",
    );
    sql(
        &app,
        "UPDATE accounts SET status = 'active' WHERE username = 'admin'",
    )
    .await;
    assert_error(
        &create_era(&app, "era-c", Some(&token)).await,
        401,
        "UNAUTHORIZED",
    );
    // A new login after enabling works.
    let fresh = login(&app, ADMIN, TEST_PASSWORD, peer(1)).await;
    assert_eq!(fresh.status.as_u16(), 200, "{}", fresh.text());
    let fresh = fresh.json()["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(create_era(&app, "era-c", Some(&fresh)).await.status, 201);

    // Password changed → the earlier token is 401.
    let other_hash = app.security.dummy_hash().to_string();
    sqlx::query("UPDATE accounts SET password_hash = $1 WHERE username = 'admin'")
        .bind(other_hash)
        .execute(&app.pool)
        .await
        .unwrap();
    assert_error(
        &create_era(&app, "era-d", Some(&fresh)).await,
        401,
        "UNAUTHORIZED",
    );

    // Account deleted → 401.
    let user = app.user_token();
    sql(&app, "DELETE FROM accounts WHERE username = 'user'").await;
    assert_error(
        &create_era(&app, "era-d", Some(&user)).await,
        401,
        "UNAUTHORIZED",
    );
}

/// Every header as sorted `(name, value)` pairs.
fn header_list(resp: &Resp) -> Vec<(String, Vec<u8>)> {
    let mut list: Vec<(String, Vec<u8>)> = resp
        .headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec()))
        .collect();
    list.sort();
    list
}

/// `GET` ignores `Authorization`: garbage, expired, or valid → identical to none (AC 8).
#[sqlx::test]
async fn reads_ignore_the_authorization_header(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app_with(
        pool_opts,
        opts,
        SecuritySettings {
            manual_clock: true,
            ..SecuritySettings::default()
        },
    )
    .await;
    assert_eq!(
        create_era(&app, "mesozoic", Some(&app.admin_token()))
            .await
            .status,
        201
    );
    let expired = issue(
        app.security.keys(),
        &Claims::new(ADMIN, 1, get_current_timestamp() - 7200),
    )
    .unwrap();
    let headers = [
        None,
        Some("Bearer garbage".to_string()),
        Some(format!("Bearer {expired}")),
        Some(format!("Bearer {}", app.admin_token())),
        Some("Basic YWRtaW46YWRtaW4=".to_string()),
        Some("Bearer".to_string()),
    ];
    let mut n = 100;
    for uri in [
        "/api/v1/eras",
        "/api/v1/eras/mesozoic",
        "/api/v1/eras/unknown-era",
        "/api/v1/species?sort=size",
        "/api/v1/nowhere",
    ] {
        let mut first: Option<Resp> = None;
        for value in &headers {
            n += 1;
            let mut req = TestRequest::get().uri(uri).peer_addr(peer(n));
            if let Some(value) = value {
                req = req.insert_header(("Authorization", value.clone()));
            }
            let resp = app.call(req).await;
            match &first {
                None => first = Some(resp),
                Some(base) => {
                    assert_eq!(resp.status, base.status, "{uri} {value:?}");
                    assert_eq!(header_list(&resp), header_list(base), "{uri} {value:?}");
                    assert_eq!(resp.body, base.body, "{uri} {value:?}");
                }
            }
        }
    }
}

/// The router's `404` / `405` come before `401` (spec 002 §4.10 steps 1→2, research R16).
#[sqlx::test]
async fn route_errors_come_before_401(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    for token in [None, Some("garbage")] {
        for uri in ["/api/v1/nowhere", "/api/v1/eras/", "/api/v2/eras", "/"] {
            let resp = send_as(&app, Method::POST, uri, json!({}), token).await;
            assert_error(&resp, 404, "ROUTE_NOT_FOUND");
        }
        for (method, uri) in [
            (Method::PUT, "/api/v1/eras"),
            (Method::DELETE, "/api/v1/periods"),
            (Method::POST, "/api/v1/continents/europe/countries"),
            (Method::PATCH, "/api/v1/species"),
            (Method::DELETE, "/api/v1/auth/login"),
        ] {
            let resp = send_as(&app, method.clone(), uri, json!({}), token).await;
            assert_error(&resp, 405, "METHOD_NOT_ALLOWED");
            assert_eq!(resp.header("www-authenticate"), None, "{method} {uri}");
        }
    }
}
