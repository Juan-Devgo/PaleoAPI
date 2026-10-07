# Plan: Spec 002 — Core CRUD

**Date:** 2026-10-06 · **Branch:** `Juan-Devgo/paleo-api-feat-crud` · **Spec:** [spec.md](./spec.md) · **Design:** [research.md](./research.md), [data-model.md](./data-model.md), [contracts/openapi.yaml](./contracts/openapi.yaml), [quickstart.md](./quickstart.md)

Spec 002 has no FR-/SC- IDs; references use spec sections (`§4.10`) and acceptance criteria (`AC 6.2.3` = §6.2 item 3). Spec 001 artifacts are cited as `001-plan`, `001-DM` (data-model), `001-R<n>` (research). Story labels `US1`–`US6` are the spec §3 table rows in order (US1 species cards, US2 filters/search, US3 paging, US4 browse tree/timeline, US5 error messages, US6 admin writes).

## Technical Context

- **Dependencies:** `bigdecimal` 0.4 (new, R1); `serde_json` 1 with `arbitrary_precision` (stack table, R1); sqlx gains feature `bigdecimal`. Nothing else: no CORS, regex, URL, date, hash, or OpenAPI crates (R6, R7, R10, R12).
- **Storage:** Spec 001 schema unchanged; no migration. Pool connections set `plan_cache_mode = force_custom_plan` (R3).
- **Queries:** `query!` family + committed `.sqlx/` for fixed shapes; `QueryBuilder` for lists; `concat!`-built static SQL for taxonomy (R3). Builds always offline (`SQLX_OFFLINE=true`).
- **Testing:** new test binary `tests/api` driving the app in-process under `#[sqlx::test]` (R11).
- **Performance:** reads keep 001-DM §6.1 shapes; statements per list are fixed regardless of page size (§Read path Embedding): species 2 + 4, countries 2 + 1, every other list 2; nested lists (`/eras/{id}/periods`, `/continents/{id}/countries`, taxonomy children) add 1 parent PK lookup (data-model §3), so 3 (countries 3 + 1).

## Constitution Check

**PASS** (pre-design and post-design).

| Principle | How this plan complies |
|---|---|
| I. Easy to Fetch | Public `GET`, CORS on every response (R6), uniform envelopes, one-fetch species cards. |
| II. Restricted Writes | Gate design and test credential: research R4; guarded by `repo_hygiene.rs` and `startup.rs` (h) (§Cases). |
| III. Performance | Every read statement (lists, counts, detail lookups, embeddings) is an 001-DM §6.1 shape and every write lookup a data-model §3 shape; `query_plans.rs` `EXPLAIN`s the exact SQL the API runs for all of them (§Cases); `performance.rs` asserts p95 < 50 ms; no N+1 (§Read path Embedding); caching headers on every `GET` response, errors included (R7). |
| IV. Test-First | Each task writes failing tests from spec §6 first; integration tests hit real PostgreSQL via the 001 harness. |
| V. Integrity | Database constraints stay authoritative; Rust pre-validation only adds complete error reporting (R9); races map back through data-model §4. |
| VI. Simplicity | One new crate (`bigdecimal`), justified in Complexity Tracking; no server-side cache; stateless process. |

**AGENTS.md deviation (§2 "compile-time checked queries"):** list/count SQL (`QueryBuilder`) and taxonomy SQL (static strings shared by 7 ranks) are runtime-checked (R3). Every such statement is executed by integration tests and `EXPLAIN`ed by `query_plans.rs`.

## Project Structure

```text
Cargo.toml                 # CHANGE: bigdecimal = { version = "0.4", default-features = false, features = ["std"] };
                           #   serde_json = { version = "1", features = ["arbitrary_precision"] }; sqlx + "bigdecimal"
.env.example               # CHANGE: SQLX_OFFLINE=true with a comment (R3)
.sqlx/                     # NEW: query metadata from `cargo sqlx prepare -- --all-targets`
AGENTS.md                  # CHANGE §3: `cargo sqlx prepare -- --all-targets` and `--check`; §5: add the `--check` gate
docs/openapi.yaml          # NEW: published copy of contracts/openapi.yaml (R12)
src/
├── lib.rs                 # CHANGE: + `pub mod api;`
├── main.rs                # CHANGE: delete stub handlers and `/` route; pool from `db::api_connect_options(opts)`;
│                          #   `HttpServer::new(move || api::app(pool.clone(), gate.clone()))`, gate = `Arc::new(DenyAll)`
├── db.rs                  # CHANGE: + `api_connect_options` (plan_cache_mode)
└── api/
    ├── mod.rs             # `app()`, `ROUTES`, route registration, middleware stack incl. CORS headers and `OPTIONS` (R6; §Request pipeline)
    ├── auth.rs            # `AdminGate`, `DenyAll`
    ├── error.rs           # `ApiError`, codes (spec §4.12), `FieldError`, rendering, 404/405 handlers, no-store on GET errors
    ├── db_error.rs        # data-model §4 map; dependents lookup + message (data-model §5)
    ├── http.rs            # write-body reader (R5), cached GET responder (R7), envelopes, `Location`
    ├── params.rs          # query-string parser: duplicates, page/limit, sort, slug/enum filters, `q`
    ├── input.rs           # `Field<T>`, object walker, shared validators (R2, R10)
    ├── decimal.rs         # `Decimal` newtype (R1)
    ├── geologic_time.rs   # eras + periods
    ├── taxonomy.rs        # `Rank` descriptors, per-rank SQL macro, generic handlers
    ├── geography.rs       # continents + countries
    └── species/
        ├── mod.rs         # handlers
        ├── input.rs       # create/patch validation incl. size
        ├── list.rs        # filters/sort → Q-S1…Q-S11
        └── card.rs        # `SpeciesCard`, batched embeddings (Q-S12)
tests/
├── api/                   # one test binary
│   ├── main.rs            # declares every module up front (empty files) so test files can be written in parallel
│   ├── support.rs         # §Test strategy helpers; `#[path = "../db/support.rs"] mod db_support;`
│   ├── tokens.rs          # `TEST_ADMIN_TOKEN`, `TEST_USER_TOKEN` (only definition); also included by `tests/startup.rs`
│   ├── global.rs  geologic_time.rs  taxonomy.rs  geography.rs  species.rs  species_filters.rs
│   └── error_map.rs  query_plans.rs  performance.rs  openapi.rs
├── startup.rs             # CHANGE: case (h) also checks the binary serves `api::app` and denies writes
└── repo_hygiene.rs        # CHANGE: + token literals absent from src/; + `DenyAll` is the only `AdminGate` in src/;
                           #   + .env.example has SQLX_OFFLINE=true
```

Public library surface used by tests:

```rust
// src/api/mod.rs
pub fn app(pool: PgPool, gate: Arc<dyn AdminGate>) -> App<impl ServiceFactory<ServiceRequest, Config = (), Response = ServiceResponse<impl MessageBody>, Error = actix_web::Error, InitError = ()>>;
pub const ROUTES: &[(&str, &[Method])];            // path under /api/v1 → methods (OpenAPI drift test)
// src/api/auth.rs
pub trait AdminGate: Send + Sync + 'static { fn check(&self, req: &HttpRequest) -> Result<(), ApiError>; }
pub struct DenyAll;
// src/api/db_error.rs
pub fn is_mapped(constraint: &str) -> bool;       // error_map.rs completeness
// list SQL builders push onto a caller-owned builder, so tests can prefix EXPLAIN
pub fn push_page_sql(qb: &mut QueryBuilder<'_, Postgres>, q: &ListQuery<F, S>);   // one pair per list module
pub fn push_count_sql(qb: &mut QueryBuilder<'_, Postgres>, q: &ListQuery<F, S>);
// fixed read statements (Q-E3, Q-P4, Q-T3, Q-K2, Q-S12, Q-S13): each module declares its SQL once through the one
// shared `fixed_query!` macro_rules! in src/api/mod.rs (literal taken as `$sql:tt`), expanding to `pub const <NAME>_SQL: &str`
// and a fn running `query_as!` on it, so tests EXPLAIN the text the API prepares; taxonomy exposes its per-rank strings through `Rank`
pub const ERA_BY_ID_SQL: &str;                     // one per statement, e.g. PERIOD_BY_ID_SQL, COUNTRY_CONTINENTS_SQL, SPECIES_PERIODS_SQL
// src/db.rs
pub fn api_connect_options(opts: PgConnectOptions) -> PgConnectOptions;
```

## Request pipeline (spec §4.10 check order)

App-level middleware, outermost first: `DefaultHeaders` (CORS headers) → `OPTIONS` short-circuit (`204`, R6) → `ErrorHandlers` (empty `405` → JSON) → router. All routes live in `web::scope("/api/v1")`; the app default service returns `404 ROUTE_NOT_FOUND`. Taxonomy registers only the six direct parent/child nested paths, so other pairings fall through to `404` (AC 6.3.6).

| Step | Where | Outcome |
|---|---|---|
| 1 | router | `404 ROUTE_NOT_FOUND`, `405 METHOD_NOT_ALLOWED` + `Allow` |
| 2 | handler: `gate.check(&req)` | `401` / `403` |
| 3 | `http::read_json(&req, payload)` → `serde_json::Value` | declared length > 65,536 → `413`; content type → `415`; streamed > 65,536 → `413`; parse → `400 INVALID_JSON` |
| 4 | path id: not a slug → `404` without a query; else Q-W1/Q-W2/Q-W3 in the write transaction | `404 <R>_NOT_FOUND` |
| 5 | body is an object (else one detail, field `""`, R2) → resource validator (pure) + database checks (R9, data-model §3) | `422 VALIDATION_FAILED`, every detail |
| 6 | write + `COMMIT`, errors through `db_error` | `409` duplicates / dependents; race `422`s |

`DELETE` skips step 3 and the step-5 body checks. `GET` handlers parse the query string first (`400`), then run the read (path `404` included).

## Read path

- **Parsing (`params.rs`):** `web::Query<Vec<(String, String)>>::from_query` (no new dependency); a malformed query string → `400`. Known keys per endpoint; a known key twice → `400`; unknown keys ignored. `page`/`limit`: ASCII digits only, ranges of spec §4.6. `sort`: per-endpoint allow-list; message lists it (AC 6.1.4). Slug filters use the slug check; `diet`/`type` use their enums; `q` trimmed, 3–64 chars, `\`, `%`, `_` escaped, bound into `ILIKE $n ESCAPE '\'` (001-plan §Notes 6).
- **Shapes:** builders emit 001-DM §6.1 filter and order text exactly (period and rank page SQL add only the parent PK join, §Embedding), including `ORDER BY lower(name) COLLATE paleo_name_sort [DESC], id`; `discovery_year` sorts use `NULLS LAST` in both directions; descending non-name sorts add `id` ascending as tie-break (spec §4.7).
- **Deviations from 001-DM §6.1 text (T059, PostgreSQL 18 plans):** Q-S6 is `s.genus_id = ANY(ARRAY(SELECT g.id FROM … WHERE <rank>.<parent>_id = $n))` instead of `IN (SELECT …)`, so the genus set is an InitPlan computed once rather than a per-row probe of the rank chain; same indexes. Q-S12 periods (`SPECIES_PERIODS_SQL`, `src/api/species/card.rs`) is `unnest($1::text[]) CROSS JOIN LATERAL (… species_periods WHERE species_id = u.id OFFSET 0)`, one `species_periods_pk` probe per id, instead of `species_id = ANY($1)` (planned as a skip scan of all of `species_periods_period_idx`); Q-S12 lineage runs on the page's distinct `genus_id`s (`genera_pk` … `domains_pk`). `query_plans.rs` verifies these texts (data-model §3).

| Endpoint | Filters → shape | Sorts → shape |
|---|---|---|
| `/eras` | — | `start_mya` Q-E1, `name` Q-E2 |
| `/eras/{id}/periods`, `/periods?era=` | era Q-P1 | Q-P1 |
| `/periods` | — | Q-P2, Q-P3 |
| `/taxonomy/{rank}` | `<parent>` Q-T2 | Q-T1 |
| `/taxonomy/{parent}/{id}/{rank}` | Q-T2 | Q-T2 |
| `/continents` | `type` Q-C1 | Q-C2 |
| `/continents/{id}/countries`, `/countries?continent=` | Q-C3 | Q-C3 |
| `/countries` | — | Q-K1 |
| `/species` | Q-S4 … Q-S11, combined with `AND` | Q-S1 … Q-S3 |

- **Execution:** single-statement reads run on the pool; multi-statement reads in one `REPEATABLE READ, READ ONLY` transaction (R8). Lists run the count first and skip the page query when `offset ≥ total_items` (deep `page` values stay cheap). `total_pages = ceil(total / limit)`, `0` when empty.
- **Embedding:** detail and list share one loader per resource, so `POST`/`PATCH` bodies equal a later `GET` (AC 6.1.16). Orders per data-model §1. Embeddings never run per item (Constitution III):

| Resource | Embedded | How (list and detail) |
|---|---|---|
| Period | `era` | joined on `eras_pk` in the page / by-id statement (Q-P1…Q-P3 page SQL, Q-P4) |
| Rank ≠ domain | parent summary | joined on `<parent>_pk` in the page / by-id statement (Q-T1, Q-T2 page SQL, Q-T3) |
| Country | `continents` | one Q-K2 link statement (`country_id = ANY($1)`) for all ids of the page, grouped in Rust |
| Species | taxonomy, periods, continents, countries | four Q-S12 statements for the page, grouped in Rust: continents and countries `species_id = ANY($1)`; periods per-id `LATERAL` probe; lineage by the page's distinct genus ids (§Read path Deviations) |

Count statements never join. The joined page SQL differs from the bare 001-DM §6.1 text, so `query_plans.rs` re-verifies it (001-DM §6.1 last paragraph).
- **Response:** `http::cached_json(&req, &body)` serializes once, sets `ETag`/`Cache-Control`, answers `304` on a match (R7).

## Write path

Every write runs in one READ COMMITTED transaction: lock/lookup (step 4) → validate (step 5) → write → re-read with the GET loader → `COMMIT` → respond (`201` + `Location`, `200`, or `204`). Normalization per data-model §1.

| Resource | Create | Patch | Delete |
|---|---|---|---|
| Era | Q-W5 | Q-W1, Q-W5, Q-W7 | Q-W2, Q-W10 |
| Period | Q-W3 (era, `404`), Q-W8, Q-W6 | Q-W1, Q-W4 (`era_id`), Q-W8, Q-W6 | Q-W2, Q-W10 |
| Rank | domain: none; child: Q-W3 (parent, `404`) | Q-W1, Q-W4 (parent) | Q-W2, Q-W10 |
| Continent | none | Q-W1, Q-W9 when `modern → prehistoric` | Q-W2, Q-W10 (countries + species) |
| Country | Q-W4 (continents with `type`) | Q-W1, Q-W4, Q-W12 | Q-W2, Q-W10 |
| Species | Q-W4 ×4, Q-W11, Q-W12 | Q-W1, Q-W4, Q-W11, Q-W12 | Q-W2 only (always `204`, AC 6.5.12) |

- Field rules come from spec §4.13 and §5; multi-field attribution from data-model §6. Messages say what is wrong and how to fix it, echoing values truncated to 32 characters (path ids to 64).
- A `PATCH` reads the stored values of data-model §2 under the Q-W1 lock and validates the merged resource (spec §4.10).
- `500`: generic body; one stderr line `paleo_api: internal error: <SQLSTATE> <constraint>`; nothing else logged.

## Test strategy

### Rules

- Tests are written before the code they cover and fail for the stated reason; a task may add `todo!()` stubs so the crate compiles (001-plan §Test strategy).
- Every `tests/api` test is `#[sqlx::test]` with `(PgPoolOptions, PgConnectOptions)` → `api_connect_options` → `app(pool, TestGate)` → `test::init_service` (R11). Only `tests/startup.rs` runs the binary.
- Writes in tests send `Authorization: Bearer <TEST_ADMIN_TOKEN>`; both token constants are defined only in `tests/api/tokens.rs`, which `support.rs` re-exports and `tests/startup.rs` includes with `#[path = "api/tokens.rs"] #[allow(dead_code)] mod tokens;`.
- Rejected-write tests also assert that a `GET` afterwards is unchanged.
- Error assertions check status, `error.code`, and, for `422`, the exact set of `details[].field`.

### Helpers (`tests/api/support.rs`)

`TestGate` (R4) · `TEST_ADMIN_TOKEN`, `TEST_USER_TOKEN` (re-exported from `tokens.rs`) · `app(pool_opts, connect_opts)` · `get(app, uri)`, `get_with(app, uri, headers)`, `send(app, method, uri, json)` (admin), `send_as(app, method, uri, json, token)`, `send_raw(app, method, uri, content_type, bytes)` · `body_json(resp)` · `assert_error(resp, status, code)` · `assert_details(resp, &[fields])` · `etag(resp)` · seeders reusing `db_support` builders (`era`, `period`, rank chain, `continent`, `country_with_continent`, `species_with_period`, `full_chain`) · `explain_with_binds(pool, qb)` and `explain_sql(pool, sql, binds)` (for the `*_SQL` constants) for `query_plans.rs`.

### Cases by file

| File | Covers | Cases |
|---|---|---|
| `src/api/decimal.rs` (unit) | R1, spec §4.13 | `251.902` ok for 3 places; `66.0000` ok, stored `66`; `66.0001` → places error; `1e3` = `1000`; text > 40 chars and `1e15` → too large; output never uses exponent (`0.000001`) |
| `src/api/input.rs` (unit) | spec §4.2, §4.13, §4.10, R10 | slug cases of 001-plan `identifiers.rs`; trim/length incl. `'ñ'×64`, U+00A0; `Field` absent/null/value; unknown field, response-only field, `id` in PATCH, `{}` PATCH → details; list duplicates and max length; URL (`HTTP://x` → stored `http://x`, `ftp://x`, `http:///x`, whitespace, 2049); integer year rejects `1905.0` (spec §5.6 types `discovery_year` as integer) |
| `src/api/params.rs` (unit) | spec §4.6–§4.8, AC 6.1.2, 6.1.4 | every AC 6.1.2 value; `+5`, `05x`; repeated known key; unknown key ignored; empty filter; sort allow-list message; `q` bounds after trim and escaping of `\%_` |
| `src/api/http.rs` (unit) | R5, R7 | content type `application/json; charset=utf-8` ok, `text/plain`/missing → 415; `If-None-Match` `*`, lists, `W/` prefix |
| `src/api/geologic_time.rs` (unit) | spec §4.10, AC 6.2.1, 6.2.9, data-model §2, §6 | merged range from stored + sent values (`start_mya` only, `end_mya` only, both, neither); `start_mya > end_mya` and equality rejected after merge; attribution of data-model §6 for range, overlap, period outside era, and era excluding periods, for each combination of sent fields |
| `src/api/species/input.rs` (unit) | spec §5.6 `size`, AC 6.5.4, 6.5.10, data-model §1, §6 | `size` `null` → all six cleared; `{}` → `422 size`; sub-object `null` = absent; unknown sub-object key; missing `min`/`max`; bound `0`, negative, 7 places; `min > max` → `size.<measure>`; `min = max` ok; only `length_m` → other pairs `NULL` |
| `src/api/auth.rs` (unit) | R4 | `DenyAll` → `401 UNAUTHORIZED` with no `Authorization`, a random bearer, and a malformed header |
| `global.rs` router | AC 6.1.15 (`404`), R3, R6, R7 | unknown route → `404 ROUTE_NOT_FOUND` with CORS headers and `no-store`; `OPTIONS` on an unknown route → `204`; a pool built with `api_connect_options` reports `SHOW plan_cache_mode` = `force_custom_plan` |
| `global.rs` reads | AC 6.1.1–6.1.6, 6.1.8, 6.1.15 (`405`), spec §4.3, §4.9 | on `/species`: defaults and limits; past-last page; `total_pages` `0` when empty; stable ties and case-insensitive text sorts; ETag/304/headers on list and detail; CORS headers on `200`, `304`, `404`; `OPTIONS` on a known route → `204`; unsupported method on a species route → `405 METHOD_NOT_ALLOWED` envelope + `Allow` |
| `global.rs` writes | AC 6.1.7, 6.1.9–6.1.17, 6.1.19–6.1.21, spec §4.10 | `null` on a required field (`name`) on `POST` and `PATCH` → `422` on that field; ETag changes after write and old tag → `200`; `OPTIONS` on write routes → `204`; `401` without token, with a random token, and on an unknown id; `403` with `TEST_USER_TOKEN`; `400`/`413` (declared and chunked)/`415`; `201` + `Location` body equals `GET`; `204` then `404`; whitespace names; `409` dependents message (count, ≤ 5 ids); check-order matrix (each adjacent pair of steps, e.g. `401` beats `413`, `404` path beats `422` incl. a non-object body, `422` beats duplicate `409`) |
| `global.rs` leaks | AC 6.1.18, R12 | no error body contains `SQL`, `constraint`, `sqlx`, `panicked`; every `ROUTES` entry answers its methods with neither `404 ROUTE_NOT_FOUND` nor `405` |
| `geologic_time.rs` reads | AC 6.1.14, 6.2.6, 6.2.7, spec §4.8 | era/period lists and sorts; details with `era` summary; `/eras/{era_id}/periods`; `/eras/{unknown}/periods` and `/eras/{non-slug}/periods` → `404 ERA_NOT_FOUND`; `/periods?era=`; `/periods?era=<well-formed unknown>` → `200`, empty `data`; unknown and non-slug ids → `404 ERA_NOT_FOUND` / `PERIOD_NOT_FOUND` |
| `geologic_time.rs` writes | AC 6.2.1–6.2.5, 6.2.8–6.2.11, spec §5.2 | all items, plus attribution of data-model §6 for range, overlap, containment, and move cases; `era_id` in the body of `POST /eras/{era_id}/periods` → `422 era_id` |
| `taxonomy.rs` reads | AC 6.1.14, 6.3.2, 6.3.5, 6.3.6, spec §4.8 | looped over all 7 ranks: list, parent filter, nested list, list and detail parent summary; `?<parent>=<well-formed unknown>` → `200`, empty `data`; unknown and non-slug ids → `404 <R>_NOT_FOUND`; nested list under an unknown parent → the parent's `404 <R>_NOT_FOUND` (all 6 nested paths); non-direct pairings for every rank pair |
| `taxonomy.rs` writes | AC 6.3.1, 6.3.3, 6.3.4, spec §5.3 | looped over all 7 ranks: create, nested create, reparent, delete with children; `<parent>_id` in a nested-create body → `422 <parent>_id` |
| `geography.rs` reads | AC 6.1.14, 6.4.2, spec §4.8 | `type` and `continent` filters; nested countries; prehistoric continent's empty country list; `/continents/{unknown}/countries` → `404 CONTINENT_NOT_FOUND`; country continents sorted by name in list and detail; `/countries?continent=<well-formed unknown>` → `200`, empty `data`; unknown and non-slug ids → `404 CONTINENT_NOT_FOUND` / `COUNTRY_NOT_FOUND` |
| `geography.rs` writes | AC 6.4.1, 6.4.3–6.4.6, spec §5.5 | all items; `continent_ids` with 11 items → `422 continent_ids` (10 ok) |
| `species.rs` reads | AC 6.5.6, 6.1.14, spec §4.2 | list item and detail card; card orders of spec §5.6; `size` `null` and partial; unknown and non-slug ids → `404 SPECIES_NOT_FOUND` |
| `species.rs` writes | AC 6.5.1–6.5.5, 6.5.10–6.5.14 | all items; size `null`, `{}`, measure `null`, unknown measure key; `discovery_year` = DB UTC year ok, +1 rejected; `PATCH` with `continent_ids: null` / `country_ids: null` → stored `[]` (spec §4.10) |
| `species_filters.rs` | AC 6.5.3 (filter), 6.5.7–6.5.9, spec §4.8, §5.6 sorts | each filter alone and combined; each rank filter; `q` literal `%%_`; `discovery_year` nulls last both directions; well-formed unknown filter id → `200`, empty `data` |
| `error_map.rs` | data-model §4 | every `public` constraint and unique index name + trigger-raised names → `is_mapped` |
| `query_plans.rs` | Constitution III, 001-DM §6.1, data-model §3 | 001 fixture + `ANALYZE`; under `enable_seqscan = off`, no Seq Scan on any resource or link table and an allowed index from 001-DM §6.1 for: every endpoint × filter × sort page SQL (incl. the joined period/rank forms) and count SQL; detail lookups Q-E3, Q-P4, Q-T3 (all 7 ranks), Q-K2, Q-S13; list embeddings Q-K2 links and Q-S12 (all four statements) with a page of ids; Q-W5, Q-W6, Q-W7, Q-W9, Q-W10. Species `q` count SQL also passes the 001-DM §6.1 Q-S11 selectivity check under `EXPLAIN (ANALYZE)` with the fixture's 3-character and longer terms: each trigram Bitmap Index Scan returns < 10% of `species` rows; `Rows Removed by Index Recheck` absent or < 10% |
| `performance.rs` | Constitution III, spec §7 | 001 fixture; species default page with `limit=100`, each species filter, `q` (fixture terms), detail, era/period/rank/continent/country lists and details; per shape 1 warm-up + one 20-run round (no retries, no best-of-N); p95 (19th of 20 sorted) < 50 ms; prints p95 per shape. Release-only gate: `#[cfg_attr(debug_assertions, ignore = "release-only: run with --release")]`, so debug `cargo test` reports it ignored; run alone as quickstart §3 `cargo test --release --test api performance -- --nocapture` (the name filter keeps other tests from loading the machine). Required before every PR; the printed p95s go in the PR description |
| `openapi.rs` | AGENTS.md §5.4, R12 | `docs/openapi.yaml` paths × methods == `ROUTES` |
| `startup.rs` (h) serve | AC 6.1.15 | raw HTTP `GET /` to the running binary → `404 ROUTE_NOT_FOUND` envelope (stub routes replaced by `api::app`) |
| `startup.rs` (h) deny | AC 6.1.9, spec §4.4 | raw HTTP `POST /api/v1/eras` with `Bearer <tokens::TEST_ADMIN_TOKEN>` to the running binary → `401` |
| `repo_hygiene.rs` | spec §4.4, R3, R4 | via `git grep --untracked … -- src` (tracked and untracked files, as `no_credentialed_urls_except_placeholders`): token literals absent from `src/`; the only `impl AdminGate for` in `src/` is `DenyAll`; `.env.example` defines `SQLX_OFFLINE=true` |

## Risks

| Risk | Mitigation |
|---|---|
| `arbitrary_precision` breaks numbers under `flatten`/untagged enums | Bodies are walked by hand (R2); DTOs avoid those attributes; decimal unit tests |
| `.sqlx/` stale or compile-time DB mismatch | `SQLX_OFFLINE=true`; `cargo sqlx prepare --check -- --all-targets` gate; quickstart §4 |
| Pre-check passes, concurrent write wins | DB constraints decide; data-model §4 maps the race to the same status |
| Generic plans drift from verified shapes | `force_custom_plan` (R3); `query_plans.rs` and `performance.rs` use the production connect options |
| Deep `page` values scan far | count first, skip page query past the end |
| actix test utilities on the sqlx tokio runtime | handlers need no actix runtime (R11); fallback documented there |
| Deadlock between concurrent writers | row lock first (Q-W1/Q-W2), links after; residual `40P01` → `500` |

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| `bigdecimal` crate | exact decimals without silent rounding (spec §4.13), unbounded sizes | f64 rounds; `rust_decimal` caps range and places (R1) |
| `serde_json/arbitrary_precision` | read the literal number text | default f64 parsing loses digits (R1) |
| Hand-written body validators | report every problem; absent vs `null` (spec §4.5, §4.10) | serde derive fails on the first error (R2) |
| `AdminGate` trait object | deny-all now, test credential outside any build, drop-in security spec | Cargo feature or env token can reach a deployed build (R4) |
| Runtime-checked list and taxonomy SQL | 13 optional filters × 6 sorts; 7 identical rank shapes | per-combination `query!` is combinatorial; catch-all predicates defeat indexes (R3) |
| Pre-validation queries duplicating DB rules | all problems in one `422` | the DB stops at the first violated constraint (R9) |
