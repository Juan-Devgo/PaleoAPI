//! `image_url` is stored, never fetched (003 AC 22; FR-037).

use std::net::TcpListener;
use std::time::Duration;

use actix_web::http::Method;
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

#[sqlx::test]
async fn a_species_image_url_is_never_fetched(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let url = format!("http://{}/probe.png", listener.local_addr().unwrap());

    let (genus, period) = seed_base(&app.pool, "ssrf").await;
    species_row(&app.pool, "ssrf-a", &genus, &period, "A", "Aus a", None).await;
    exec(
        &app.pool,
        &format!("UPDATE species SET image_url = '{url}' WHERE id = 'ssrf-a'"),
    )
    .await
    .unwrap();

    // Reads of a stored URL.
    assert_eq!(
        get(&app, "/api/v1/species/ssrf-a").await.status.as_u16(),
        200
    );
    assert_eq!(get(&app, "/api/v1/species").await.status.as_u16(), 200);
    // A write that sets the URL.
    let patched = send(
        &app,
        Method::PATCH,
        "/api/v1/species/ssrf-a",
        json!({ "image_url": url }),
    )
    .await;
    assert_eq!(patched.status.as_u16(), 200, "{}", patched.text());
    assert_eq!(patched.json()["data"]["image_url"], url);

    std::thread::sleep(Duration::from_millis(500));
    match listener.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok((_, from)) => panic!("the API connected to the image_url host from {from}"),
        Err(e) => panic!("accept: {e}"),
    }
}
