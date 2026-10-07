# Research — Spec 002

**Date:** 2026-10-06. Builds on Spec 001 research (R1–R16, cited as `001-R<n>`). "Checked" = read in the crate source cached in `~/.cargo/registry` (sqlx-cli 0.9.0, sqlx-macros-core 0.9.0, actix-web 4.15.0, serde_json 1.0.151).

## R1. Exact decimals end to end (spec §4.13, §5.1, §5.6)

- **Decision:** `bigdecimal` 0.4 (no default features except `std`) + sqlx feature `bigdecimal`; `serde_json` with feature `arbitrary_precision`. Input: read the body's `serde_json::Number` text with `Number::as_str()` and parse it with `BigDecimal::from_str`. Output: a `Decimal` newtype serializes as a JSON number by parsing `BigDecimal::to_plain_string()` into a `serde_json::Number` (keeps the text under `arbitrary_precision`).
- **Decimal places and normalized output (spec §4.13):** places are counted on `normalized()`, which matches the DB rule `v = round(v, n)` (001 data-model §2.1); the normalized value is what is stored and returned.
- **Bound check order (spec §4.13 bound):** the 40-character text check runs on `Number::as_str()` before `BigDecimal::from_str`, and the `|v| ≥ 10^15` check runs before any other arithmetic, so `1e1000000000` never reaches PostgreSQL (`22003` → `500`) or a formatter. Messages echo the received text truncated to 32 characters, never `to_plain_string()` of the input.
- **Why:** f64 parsing silently rounds (`4600.0000000000000001` → `4600.0`, accepted), which the spec forbids. `rust_decimal` caps near 7.9e28 and 28 places (001 plan §Notes 6). `arbitrary_precision` keeps the literal text, so places and bounds are judged exactly. `to_plain_string()` never emits exponent notation (bigdecimal docs).
- **Constraint:** `arbitrary_precision` is crate-graph-wide and breaks numbers inside `#[serde(flatten)]` and untagged/internally tagged enums. Request bodies are never deserialized with derive (R2); response DTOs do not use those attributes.
- **Rejected:** `rust_decimal` (range cap); numeric as `String` end to end (hand-written comparison for bounds, `start > end`, `min ≤ max`); `Number::from_string_unchecked` for output (`#[doc(hidden)]`, checked in serde_json source); `bigdecimal`'s `serde-json` helpers (`arbitrary_precision` module validates against its own scale limit, not ours).

## R2. Request body validation that reports every problem (spec §4.5, §4.10)

- **Decision:** the body reader (R5) parses to `serde_json::Value` and stops there; the hand-written per-resource validators (check-order step 5, after the path lookup) require an object, walk the `Map`, collect every `FieldError { field, message }`, and produce typed input. Each field is read as `Field<T> = Absent | Null | Value(T)` so `PATCH` distinguishes omitted, `null`, and a value.
- **Field paths:** top-level name (`end_mya`); nested `size.length_m.min`; list items by field name (`period_ids`) with the item named in the message. A body that is valid JSON but not an object is a step-5 `422` with one detail whose `field` is `""` (the whole body), so an unknown path resource still answers `404` first (spec §4.10).
- **Why:** derive-based `Deserialize` stops at the first error and cannot express absent-vs-null without wrappers; `deny_unknown_fields` reports only the first unknown key. Spec AC 6.1.12 needs all problems at once.
- **Rejected:** `serde` derive + `deny_unknown_fields` (fail-fast); `validator`/`garde` (new dependencies, still run after a fail-fast deserialize).
- **Accepted limitation:** duplicate keys in one JSON object keep the last value (serde_json `Map` behavior).

## R3. Query execution: compile-time checks, dynamic lists, plan stability

- **Decision:**
  1. Fixed-shape statements (by-id reads, inserts, updates, deletes, link replacement, embeddings, reference and domain-rule lookups) use `sqlx::query!`/`query_as!`/`query_scalar!`, with metadata committed in `.sqlx/` via `cargo sqlx prepare -- --all-targets`.
  2. List and count statements are assembled with `sqlx::QueryBuilder` from static fragments chosen by enums; every user value is `push_bind`. Placeholders carry explicit casts (`$n::text`, `$n::bigint`) so the same SQL can be `EXPLAIN`ed with binds in tests.
  3. Taxonomy statements are `&'static str` built per rank with `concat!` in a `macro_rules!`, run with runtime `query_as`.
  4. Every API pool connection sets `plan_cache_mode = force_custom_plan`.
- **`.sqlx` and the test command (checked):** `DATABASE_URL` takes precedence over `.sqlx`, and tests run with `DATABASE_URL` = the `postgres` maintenance DB (001-R14), which has no schema; `query!` would then fail to compile. `.env.example` sets `SQLX_OFFLINE=true` so builds always use `.sqlx`. `cargo sqlx prepare` forces `SQLX_OFFLINE=false` for its own build (`sqlx-cli` `prepare.rs`: `.env("SQLX_OFFLINE", "false")`), so it still reads the dev DB. `cargo sqlx prepare --check -- --all-targets` is a quality gate.
- **Why custom plans:** sqlx prepares named statements per connection; after five executions PostgreSQL may switch to a generic plan that cannot see the `q` pattern or a skewed `diet` value, losing the plans verified in 001 data-model §6.1. Planning cost per request is well under the 50 ms budget.
- **Rejected:** one static `query!` per filter×sort combination (species alone has 13 optional filters × 6 sorts); `($1 IS NULL OR col = $1)` catch-all filters (generic plans ignore the index); `query!` per rank (7 × ~10 literals with identical shape); runtime queries everywhere (contradicts AGENTS.md §2 without need).

## R4. Write access before the security spec (spec §4.4, AC 6.1.9)

- **Decision:** a trait `AdminGate { fn check(&self, req: &HttpRequest) -> Result<(), ApiError> }` stored as `web::Data<dyn AdminGate>`. The library ships one implementation, `DenyAll`, which always returns `401 UNAUTHORIZED`; `main.rs` installs it unconditionally. Integration tests construct the app with `TestGate`, defined only in `tests/api/support.rs`: `Authorization: Bearer <TEST_ADMIN_TOKEN>` passes, `Bearer <TEST_USER_TOKEN>` returns `403`, anything else `401`.
- **Why:** the test credential exists only in a test crate, so no build of the binary or library can accept it; the security spec replaces `DenyAll` without touching handlers. `403` stays exercised.
- **Rejected:** Cargo feature `test-auth` (can be enabled in a release build); env-var token (present in deployed builds, violates §4.4); `#[cfg(test)]` (not set for `tests/` integration crates); leaving writes unrouted until the security spec (spec requires the full write contract now).

## R5. Check order (spec §4.10 "Check order")

- **Decision:** write handlers take `HttpRequest` + `web::Payload` (the raw stream, whose extractor never fails — checked, actix-web `types/payload.rs`) and run the steps explicitly: gate → body (`Content-Length` > 65,536 → `413`; `Content-Type` essence ≠ `application/json` → `415`; stream with a 65,536-byte cap → `413`; parse → `400 INVALID_JSON`) → path resource lookup (`404`) → validation (`422`) → write (`409` from the database).
- **404/405:** routing runs before handlers, so `404 ROUTE_NOT_FOUND` (app default service) and `405` precede the gate. Actix resources answer unmatched methods with an empty `405` plus `Allow` (checked, `resource.rs`); the built-in `middleware::ErrorHandlers` rewrites empty `405` bodies into the `METHOD_NOT_ALLOWED` envelope, keeping `Allow`.
- **Rejected:** `web::Json<T>` extractors (fail before the gate runs, so `400` would beat `401`); a gate middleware on scopes or resources (runs before method dispatch, so `401` would beat `405`).

## R6. CORS (spec §4.3, AC 6.1.8)

- **Decision:** no CORS crate. `middleware::DefaultHeaders` adds `Access-Control-Allow-Origin: *` and `Access-Control-Expose-Headers: ETag` to every response (errors and `304` included). An app-level `wrap_fn` answers every `OPTIONS` request, before routing, with `204` + `Access-Control-Allow-Methods: GET`, `Access-Control-Allow-Headers: If-None-Match`, `Access-Control-Max-Age: 86400`.
- **Why:** `If-None-Match` is not a CORS-safelisted request header, so a script sending it triggers a preflight. Cross-origin writes are not granted: the spec opens only `GET` (§4.3); the security spec decides write CORS together with credentials.
- **Rejected:** `actix-cors` (a dependency for three fixed headers and one preflight answer; it validates preflights instead of always answering them).

## R7. HTTP caching (spec §4.9, AC 6.1.6–6.1.7)

- **Decision:** every `GET` serializes the full JSON body to bytes, hashes them with `std::hash::DefaultHasher`, and sends `ETag: "<16 hex>"` (strong) + `Cache-Control: public, max-age=300`. `If-None-Match` uses the weak comparison of RFC 9110 §13.1.2 (`W/` ignored, comma lists, `*` matches) → `304` with no body and the same headers. `GET` error responses carry `Cache-Control: no-store` (Constitution III requires caching headers on every `GET` response).
- **Why:** hashing the bytes covers every change the spec lists (items, embedded data, order, totals) with no versioning columns or triggers. Data is read on every request, so the server holds no cache (spec §4.9, Constitution VI).
- **Accepted:** `DefaultHasher` output may change with a Rust release; that only causes one cache miss after a deploy.
- **Rejected:** `updated_at`/version columns (schema change, misses embedded-data changes); `sha2`/`xxhash` crates (dependency for a non-security hash).

## R8. Read consistency

- **Decision:** a `GET` that runs more than one statement (count + page + embeddings, existence check + nested list) runs in `BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY`. Writes use READ COMMITTED (001 plan §Notes 2).
- **Why:** without one snapshot, a concurrent delete between the page and the embedding query yields a species card with no periods, and totals can disagree with `data`. The RR guard of 001-R10 fires only in write triggers, so read-only RR is safe.

## R9. Domain rules: pre-validation plus database authority (spec §4.5, §4.10)

- **Decision:** inside the write transaction, after the path lookup: (1) pure validation of the merged resource; (2) database-backed checks for fields that passed (1): referenced ids exist, era/period overlap, containment, modern-only continents, type change with linked countries, current UTC year from the DB clock; (3) any detail → `422` with all of them; (4) write. Database errors from (4) are races or duplicates, mapped by SQLSTATE + constraint (data-model §4).
- **Why:** the database stops at the first violated constraint, so it cannot report every problem; pre-checks provide the full list, and the database stays the authority under concurrency (Constitution V). Duplicates are left to `23505` so AC 6.1.21 (`422` beats `409`) holds and case-folding stays with PostgreSQL's `lower()` (001 plan §Notes 4).
- **Year source:** `SELECT extract(year FROM now() AT TIME ZONE 'UTC')` in the same transaction, the clock the trigger uses (001-R12); no date crate.

## R10. Text, slugs, URLs

- **Decision:** hand-written checks. Trim with `str::trim()` and count `chars()`; this equals the DB's `\s` and `char_length` under builtin `C.UTF-8` (001-R7). Slug: byte scan for `^[a-z0-9]+(-[a-z0-9]+)*$`, 2–64. `image_url`: case-insensitive `http://` or `https://`, non-empty authority (next char not `/`, `?`, `#`), no whitespace or control characters, ≤ 2048 chars; the scheme is lowercased before storage (001 plan §Notes 5).
- **Rejected:** `regex`, `url` crates (dependencies for one fixed pattern and a prefix check).

## R11. Testing the HTTP contract

- **Decision:** `tests/api/` is one test binary. Each test is `#[sqlx::test]` taking `(PgPoolOptions, PgConnectOptions)`, builds the pool with `paleo_api::db::api_connect_options` (same tuning as production, R3), and drives `paleo_api::api::app(..)` through `actix_web::test::init_service` + `call_service` on the sqlx tokio runtime (handlers only await sqlx futures; no actix `System` needed). Table builders and `explain` are reused from `tests/db/support.rs` via `#[path]`.
- **Query plans:** the list-SQL builders and the fixed read statements' SQL constants are public (plan §Project Structure), so `tests/api/query_plans.rs` `EXPLAIN`s the exact SQL the API runs (with binds) on the 001 fixture under `enable_seqscan = off` (001-R15).
- **Latency:** `tests/api/performance.rs` is a plain `#[sqlx::test]` that loads the 001 fixture (`index_dataset.sql`) through `exec_script`, then times in-process `call_service` (server-side, no network) per read shape: 1 warm-up + one 20-run round, release-only via `#[cfg_attr(debug_assertions, ignore = ...)]`, run under `--release` (plan §Cases `performance.rs`).
- **Risk:** if a handler ever needs actix's runtime (`web::block`, `actix_rt::spawn`), tests must switch to `#[actix_web::test]` with a manually created test database; no current design element needs it.

## R12. OpenAPI

- **Decision:** OpenAPI 3.1, hand-written YAML, explicit path per resource and rank (code generators need concrete schemas). Design copy: `contracts/openapi.yaml`; implementation publishes it as `docs/openapi.yaml`, the living description later specs update (AGENTS.md §5.4). `tests/api/openapi.rs` extracts `paths` and methods from `docs/openapi.yaml` by line pattern (no YAML dependency) and asserts both directions against the route table the app exposes (`api::ROUTES`).
- **Rejected:** `utoipa` (proc-macro dependency, annotations on every handler); serving the document from the API (not in the spec).

## Sources

- sqlx workspace manifest (bigdecimal 0.4, feature names): https://raw.githubusercontent.com/transact-rs/sqlx/main/Cargo.toml
- sqlx-cli README (prepare, `--check`, `DATABASE_URL` precedence, `SQLX_OFFLINE`): https://raw.githubusercontent.com/transact-rs/sqlx/main/sqlx-cli/README.md
- `#[sqlx::test]` (pool/connect options arguments, fixtures): https://docs.rs/sqlx/latest/sqlx/attr.test.html
- serde_json `Number` under `arbitrary_precision`: https://raw.githubusercontent.com/serde-rs/json/master/src/number.rs
- bigdecimal 0.4.11 API (`to_plain_string`, `normalized`): https://docs.rs/bigdecimal/latest/bigdecimal/struct.BigDecimal.html
- bigdecimal features and serde helpers: https://docs.rs/crate/bigdecimal/latest/source/Cargo.toml.orig, https://raw.githubusercontent.com/akubera/bigdecimal-rs/trunk/src/impl_serde.rs
- PostgreSQL 18 `plan_cache_mode`: https://www.postgresql.org/docs/18/runtime-config-query.html#GUC-PLAN-CACHE-MODE
- PostgreSQL 18 `PREPARE` (custom vs generic plans): https://www.postgresql.org/docs/18/sql-prepare.html
- RFC 9110 §13.1.2 If-None-Match, §8.8.3 ETag: https://www.rfc-editor.org/rfc/rfc9110
- Fetch standard, CORS-safelisted request headers: https://fetch.spec.whatwg.org/#cors-safelisted-request-header
- actix-web 4 `middleware::ErrorHandlers`, `DefaultHeaders`: https://docs.rs/actix-web/4/actix_web/middleware/index.html
- OpenAPI 3.1.1: https://spec.openapis.org/oas/v3.1.1.html
