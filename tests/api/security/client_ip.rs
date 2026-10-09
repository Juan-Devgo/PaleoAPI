//! Client identification for limits (003 AC 13; FR-019, research R5).

use std::net::SocketAddr;

use actix_web::test::TestRequest;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

/// Burst 2 and a manual clock, so no bucket refills during a test.
fn settings(trusted: &str) -> SecuritySettings {
    SecuritySettings {
        config: security_env(&[
            ("RATE_LIMIT_PER_MINUTE", "6"),
            ("RATE_LIMIT_BURST", "2"),
            ("TRUSTED_PROXIES", trusted),
        ]),
        manual_clock: true,
        seed: false,
        ..SecuritySettings::default()
    }
}

async fn status_from(app: &TestApp, from: SocketAddr, forwarded: Option<&str>) -> u16 {
    let mut req = TestRequest::get().uri("/api/v1/eras").peer_addr(from);
    if let Some(value) = forwarded {
        req = req.insert_header(("X-Forwarded-For", value));
    }
    app.call(req).await.status.as_u16()
}

fn v6(addr: &str) -> SocketAddr {
    SocketAddr::new(addr.parse().unwrap(), 40_000)
}

#[sqlx::test]
async fn forwarded_for_from_an_untrusted_peer_is_ignored(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app_with(pool_opts, opts, settings("")).await;
    for forwarded in ["198.51.100.1", "198.51.100.2"] {
        assert_eq!(status_from(&app, peer(1), Some(forwarded)).await, 200);
    }
    // A third spoofed address is still the same peer.
    assert_eq!(status_from(&app, peer(1), Some("198.51.100.3")).await, 429);
    assert_eq!(status_from(&app, peer(2), None).await, 200);
}

#[sqlx::test]
async fn forwarded_for_from_a_trusted_proxy_names_the_client(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app_with(pool_opts, opts, settings("10.0.0.0/8")).await;
    for _ in 0..2 {
        assert_eq!(status_from(&app, peer(1), Some("198.51.100.1")).await, 200);
    }
    assert_eq!(status_from(&app, peer(1), Some("198.51.100.1")).await, 429);
    // Another client behind the same proxy has its own limit.
    assert_eq!(status_from(&app, peer(1), Some("198.51.100.2")).await, 200);
    // The proxy itself is not limited by what its clients used.
    assert_eq!(status_from(&app, peer(1), None).await, 200);
    // A client address prepended by the caller is not trusted: the rightmost
    // untrusted hop counts.
    assert_eq!(
        status_from(&app, peer(1), Some("203.0.113.9, 198.51.100.1")).await,
        429
    );
}

#[sqlx::test]
async fn addresses_in_one_ipv6_prefix_share_a_limit(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app_with(pool_opts, opts, settings("")).await;
    assert_eq!(status_from(&app, v6("2001:db8:1:2::1"), None).await, 200);
    assert_eq!(
        status_from(&app, v6("2001:db8:1:2:ffff::9"), None).await,
        200
    );
    assert_eq!(status_from(&app, v6("2001:db8:1:2:1::5"), None).await, 429);
    // Another /64 is another client.
    assert_eq!(status_from(&app, v6("2001:db8:1:3::1"), None).await, 200);
}
