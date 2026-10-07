//! Taxonomy ranks (spec §5.3; AC 6.3).

use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

/// A rank: plural path segment, singular key, code stem, parent index.
pub struct R {
    pub plural: &'static str,
    pub singular: &'static str,
    pub code: &'static str,
    pub parent: Option<usize>,
}

pub const RANKS: [R; 7] = [
    R {
        plural: "domains",
        singular: "domain",
        code: "DOMAIN",
        parent: None,
    },
    R {
        plural: "kingdoms",
        singular: "kingdom",
        code: "KINGDOM",
        parent: Some(0),
    },
    R {
        plural: "phyla",
        singular: "phylum",
        code: "PHYLUM",
        parent: Some(1),
    },
    R {
        plural: "classes",
        singular: "class",
        code: "CLASS",
        parent: Some(2),
    },
    R {
        plural: "orders",
        singular: "order",
        code: "ORDER",
        parent: Some(3),
    },
    R {
        plural: "families",
        singular: "family",
        code: "FAMILY",
        parent: Some(4),
    },
    R {
        plural: "genera",
        singular: "genus",
        code: "GENUS",
        parent: Some(5),
    },
];

fn summary(id: &str) -> Value {
    json!({ "id": id, "name": id })
}

/// The representation of `<prefix>-<plural>` as seeded by `genus_chain`.
fn item(prefix: &str, r: &R) -> Value {
    let id = format!("{prefix}-{}", r.plural);
    match r.parent {
        None => summary(&id),
        Some(p) => {
            let mut v = summary(&id);
            v[RANKS[p].singular] = summary(&format!("{prefix}-{}", RANKS[p].plural));
            v
        }
    }
}

// ---------------------------------------------------------------- reads

#[sqlx::test]
async fn lists_filters_and_details_for_every_rank(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    genus_chain(&app.pool, "a").await;
    genus_chain(&app.pool, "b").await;
    for r in &RANKS {
        let base = format!("/api/v1/taxonomy/{}", r.plural);
        let resp = get(&app, &base).await;
        assert_eq!(resp.status.as_u16(), 200, "{base}: {}", resp.text());
        assert_eq!(
            resp.json()["data"],
            json!([item("a", r), item("b", r)]),
            "{base}"
        );
        assert_eq!(resp.header("cache-control"), Some("public, max-age=300"));
        assert_eq!(
            ids(&get(&app, &format!("{base}?sort=-name")).await),
            [format!("b-{}", r.plural), format!("a-{}", r.plural)]
        );
        assert_error(
            &get(&app, &format!("{base}?sort=start_mya")).await,
            400,
            "INVALID_QUERY_PARAMETER",
        );

        let detail = get(&app, &format!("{base}/a-{}", r.plural)).await;
        assert_eq!(detail.status.as_u16(), 200, "{}", detail.text());
        assert_eq!(detail.json(), json!({ "data": item("a", r) }));
        if r.parent.is_none() {
            assert_eq!(
                detail.json()["data"].as_object().unwrap().len(),
                2,
                "a domain has no parent field"
            );
        }

        let resp = get(&app, &format!("{base}/unknown-id")).await;
        assert_error(&resp, 404, &format!("{}_NOT_FOUND", r.code));
        let resp = get(&app, &format!("{base}/Not_A_Slug")).await;
        assert_error(&resp, 404, &format!("{}_NOT_FOUND", r.code));

        if let Some(p) = r.parent {
            let parent = &RANKS[p];
            let key = parent.singular;
            let filtered = get(&app, &format!("{base}?{key}=a-{}", parent.plural)).await;
            assert_eq!(
                filtered.json()["data"],
                json!([item("a", r)]),
                "{base}?{key}"
            );
            assert!(
                ids(&get(&app, &format!("{base}?{key}=unknown-parent")).await).is_empty(),
                "{base}?{key}=unknown"
            );
            assert_error(
                &get(&app, &format!("{base}?{key}=Bad_Id")).await,
                400,
                "INVALID_QUERY_PARAMETER",
            );

            let nested = format!(
                "/api/v1/taxonomy/{}/b-{}/{}",
                parent.plural, parent.plural, r.plural
            );
            let resp = get(&app, &nested).await;
            assert_eq!(resp.json()["data"], json!([item("b", r)]), "{nested}");
            assert_eq!(
                pagination(&resp),
                json!({ "page": 1, "limit": 20, "total_items": 1, "total_pages": 1 })
            );

            for bad_parent in ["unknown-parent", "NOT_A_SLUG"] {
                let uri = format!(
                    "/api/v1/taxonomy/{}/{bad_parent}/{}",
                    parent.plural, r.plural
                );
                assert_error(
                    &get(&app, &uri).await,
                    404,
                    &format!("{}_NOT_FOUND", parent.code),
                );
            }
        }
    }
}

#[sqlx::test]
async fn children_of_one_parent_only(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    genus_chain(&app.pool, "a").await;
    exec(&app.pool, "INSERT INTO domains VALUES ('lonely', 'Lonely')")
        .await
        .unwrap();
    exec(
        &app.pool,
        "INSERT INTO kingdoms VALUES ('zz-kingdom', 'Another kingdom', 'a-domains'), \
         ('ab-kingdom', 'Above', 'a-domains')",
    )
    .await
    .unwrap();
    assert_eq!(
        ids(&get(&app, "/api/v1/taxonomy/domains/a-domains/kingdoms").await),
        ["a-kingdoms", "ab-kingdom", "zz-kingdom"]
    );
    assert!(ids(&get(&app, "/api/v1/taxonomy/domains/lonely/kingdoms").await).is_empty());
}

#[sqlx::test]
async fn non_direct_pairings_are_route_not_found(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    genus_chain(&app.pool, "a").await;
    for (pi, parent) in RANKS.iter().enumerate() {
        for child in &RANKS {
            if child.parent == Some(pi) {
                continue;
            }
            let uri = format!(
                "/api/v1/taxonomy/{}/a-{}/{}",
                parent.plural, parent.plural, child.plural
            );
            assert_error(&get(&app, &uri).await, 404, "ROUTE_NOT_FOUND");
        }
    }
    assert_error(&get(&app, "/api/v1/taxonomy").await, 404, "ROUTE_NOT_FOUND");
    assert_error(
        &get(&app, "/api/v1/taxonomy/species").await,
        404,
        "ROUTE_NOT_FOUND",
    );
}
