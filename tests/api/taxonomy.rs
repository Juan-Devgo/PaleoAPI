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

// ---------------------------------------------------------------- writes

use actix_web::http::Method;

#[sqlx::test]
async fn domain_create_patch_delete(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/taxonomy/domains",
        json!({ "id": "eukaryota", "name": " Eukaryota " }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(
        resp.header("location"),
        Some("/api/v1/taxonomy/domains/eukaryota")
    );
    assert_eq!(
        resp.json(),
        json!({ "data": { "id": "eukaryota", "name": "Eukaryota" } })
    );
    assert_eq!(
        resp.json(),
        get(&app, "/api/v1/taxonomy/domains/eukaryota").await.json()
    );

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/taxonomy/domains",
        json!({ "id": "x1", "name": "X", "parent_id": "y" }),
    )
    .await;
    assert_details(&resp, &["parent_id"]);
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/taxonomy/domains",
        json!({ "id": "eukaryota", "name": "Other" }),
    )
    .await;
    assert_error(&resp, 409, "DOMAIN_ALREADY_EXISTS");
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/taxonomy/domains",
        json!({ "id": "other", "name": "EUKARYOTA" }),
    )
    .await;
    assert_error(&resp, 409, "DOMAIN_ALREADY_EXISTS");

    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/taxonomy/domains/eukaryota",
        json!({ "domain_id": "x" }),
    )
    .await;
    assert_details(&resp, &["domain_id"]);
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/taxonomy/domains/eukaryota",
        json!({ "name": "Eukarya" }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(resp.json()["data"]["name"], "Eukarya");

    assert_eq!(
        delete(&app, "/api/v1/taxonomy/domains/eukaryota")
            .await
            .status
            .as_u16(),
        204
    );
    assert_error(
        &delete(&app, "/api/v1/taxonomy/domains/eukaryota").await,
        404,
        "DOMAIN_NOT_FOUND",
    );
}

#[sqlx::test]
async fn every_child_rank_create_reparent_delete(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    genus_chain(&app.pool, "a").await;
    genus_chain(&app.pool, "b").await;
    for r in RANKS.iter().skip(1) {
        let parent = &RANKS[r.parent.unwrap()];
        let parent_field = format!("{}_id", parent.singular);
        let nested = format!(
            "/api/v1/taxonomy/{}/a-{}/{}",
            parent.plural, parent.plural, r.plural
        );
        let id = format!("new-{}", r.plural);
        let item_uri = format!("/api/v1/taxonomy/{}/{id}", r.plural);

        // a parent id in the body of a nested create is rejected
        let mut body = json!({ "id": id, "name": format!("New {}", r.plural) });
        body[&parent_field] = json!(format!("a-{}", parent.plural));
        assert_details(
            &send(&app, Method::POST, &nested, body).await,
            &[&parent_field],
        );

        // unknown parent in the path: the parent's 404
        let unknown = format!(
            "/api/v1/taxonomy/{}/nope-parent/{}",
            parent.plural, r.plural
        );
        let resp = send(
            &app,
            Method::POST,
            &unknown,
            json!({ "id": id, "name": "X" }),
        )
        .await;
        assert_error(&resp, 404, &format!("{}_NOT_FOUND", parent.code));

        // only domains are created without a parent
        let resp = send(
            &app,
            Method::POST,
            &format!("/api/v1/taxonomy/{}", r.plural),
            json!({ "id": id, "name": "X" }),
        )
        .await;
        assert_error(&resp, 405, "METHOD_NOT_ALLOWED");

        let resp = send(
            &app,
            Method::POST,
            &nested,
            json!({ "id": id, "name": format!("New {}", r.plural) }),
        )
        .await;
        assert_eq!(resp.status.as_u16(), 201, "{nested}: {}", resp.text());
        assert_eq!(resp.header("location"), Some(item_uri.as_str()));
        assert_eq!(
            resp.json()["data"][parent.singular],
            json!({ "id": format!("a-{}", parent.plural), "name": format!("a-{}", parent.plural) })
        );
        assert_eq!(resp.json(), get(&app, &item_uri).await.json());

        // reparent
        for (value, fields) in [
            (json!("nope-parent"), vec![parent_field.as_str()]),
            (json!(null), vec![parent_field.as_str()]),
            (json!("Bad"), vec![parent_field.as_str()]),
        ] {
            let mut body = json!({});
            body[&parent_field] = value;
            assert_details(&send(&app, Method::PATCH, &item_uri, body).await, &fields);
        }
        let mut body = json!({ "name": format!("Moved {}", r.plural) });
        body[&parent_field] = json!(format!("b-{}", parent.plural));
        let resp = send(&app, Method::PATCH, &item_uri, body).await;
        assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
        assert_eq!(
            resp.json()["data"][parent.singular]["id"],
            format!("b-{}", parent.plural)
        );
        assert_eq!(resp.json(), get(&app, &item_uri).await.json());
        assert_eq!(
            ids(&get(
                &app,
                &format!(
                    "/api/v1/taxonomy/{}/b-{}/{}",
                    parent.plural, parent.plural, r.plural
                )
            )
            .await),
            [format!("b-{}", r.plural), id.clone()]
        );

        // delete protection on the parent, plain delete on the leaf
        let resp = delete(
            &app,
            &format!("/api/v1/taxonomy/{}/a-{}", parent.plural, parent.plural),
        )
        .await;
        assert_error(&resp, 409, &format!("{}_HAS_DEPENDENTS", parent.code));
        assert!(
            resp.text().contains(&format!("a-{}", r.plural)),
            "{}",
            resp.text()
        );
        assert_eq!(delete(&app, &item_uri).await.status.as_u16(), 204);
        assert_error(
            &get(&app, &item_uri).await,
            404,
            &format!("{}_NOT_FOUND", r.code),
        );
    }
}

#[sqlx::test]
async fn genus_with_species_cannot_be_deleted(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let (genus, period) = seed_base(&app.pool, "g").await;
    species_row(&app.pool, "rex", &genus, &period, "Rex", "Rex rex", None).await;
    let resp = delete(&app, &format!("/api/v1/taxonomy/genera/{genus}")).await;
    assert_error(&resp, 409, "GENUS_HAS_DEPENDENTS");
    assert!(
        resp.text().contains("1 species") && resp.text().contains("rex"),
        "{}",
        resp.text()
    );
}

#[sqlx::test]
async fn reparenting_a_genus_changes_its_species(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let (genus, period) = seed_base(&app.pool, "g").await;
    genus_chain(&app.pool, "h").await;
    species_row(&app.pool, "rex", &genus, &period, "Rex", "Rex rex", None).await;
    let before = get(&app, "/api/v1/species/rex").await;
    let resp = send(
        &app,
        Method::PATCH,
        &format!("/api/v1/taxonomy/genera/{genus}"),
        json!({ "family_id": "h-families" }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    let after = get_with(
        &app,
        "/api/v1/species/rex",
        &[("If-None-Match", &etag(&before))],
    )
    .await;
    assert_eq!(after.status.as_u16(), 200);
    assert_ne!(etag(&after), etag(&before));
    let taxonomy = after.json()["data"]["taxonomy"].clone();
    assert_eq!(taxonomy["family"]["id"], "h-families");
    assert_eq!(taxonomy["domain"]["id"], "h-domains");
    assert_eq!(taxonomy["genus"]["id"], genus);
}
