# Plan: Spec 001 — Database Foundation

**Date:** 2026-10-03 · **Spec:** [spec.md](./spec.md) · **Design:** [research.md](./research.md), [data-model.md](./data-model.md), [quickstart.md](./quickstart.md)

## Technical Context

- **Toolchain:** Rust ≥ 1.94 (sqlx 0.9 MSRV); `Cargo.toml` gets `rust-version = "1.94"`.
- **Dependencies:** `sqlx` 0.9 (`default-features = false`; `runtime-tokio`, `postgres`, `macros`, `migrate`). Dev-only `tokio` 1 (`rt`, `macros`, `time`). Tool: `sqlx-cli` 0.9. (R2)
- **Storage:** PostgreSQL 18, `postgres:18-trixie`, UTF8 + builtin `C.UTF-8` default, ICU collation `paleo_name_sort` for name sorts only, `pg_trgm`. (R1, R7, R13)
- **Testing:** `#[sqlx::test]` per-test databases on the compose server, harness master = `postgres` DB (R14). Binary tests via `CARGO_BIN_EXE_paleo_api`.
- **Structure:** the crate gains a library target (`src/lib.rs`) so integration tests reach startup and migration code.
- **Scale:** 12 resource + 4 link tables, 6 migrations; verification dataset in data-model §9.

## Constitution Check

PASS. Additions are justified in Complexity Tracking. Rejected as not needed: `dotenvy`, TLS features, `serde_json`, testcontainers, `btree_gist`, `citext`, configurable startup timeout.

**AGENTS.md deviation:** "compile-time checked queries" — this feature's two catalog lookups (`to_regclass`, collation version) use runtime `sqlx::query_scalar`, so builds need no database or `.sqlx` cache; Spec 002 introduces `query!` with `cargo sqlx prepare`. Tests use runtime `sqlx::query` because they send invalid writes.

## Project Structure

No `contracts/`: no HTTP surface; the only external interface is §Startup contract.

```text
Cargo.toml                 # CHANGE: rust-version; sqlx; tokio dev-dep
.gitignore                 # CHANGE: add `.env` (keep `/target`)
.gitattributes             # NEW: `migrations/*.sql text eol=lf` (R3)
.env.example               # NEW: placeholders only (quickstart §2), comment: TEST_DATABASE_URL is the harness master, never the dev DB
compose.yaml               # NEW: §Local database service
AGENTS.md                  # CHANGE §3 only: `docker compose up -d --wait db`; `cp .env.example .env` + `set -a; . ./.env; set +a` before `sqlx migrate run`;
                           #   test command `DATABASE_URL="$TEST_DATABASE_URL" cargo test` + warning against plain `cargo test` with dev DATABASE_URL;
                           #   `cargo clippy --all-targets -- -D warnings`; sqlx-cli install line
README.md                  # NEW: one-liner, Getting started (quickstart §2–§5), links to constitution, quickstart, AGENTS.md; only `change-me` placeholders
build.rs                   # NEW: `cargo:rerun-if-changed=migrations` (as `sqlx migrate build-script`) so migrate! re-embeds
migrations/                # NEW: 6 reversible pairs, data-model §7
src/
├── lib.rs                 # NEW: `pub mod config; pub mod db;`
├── config.rs              # NEW: database_options_from_env (+ unit tests)
├── db.rs                  # NEW: MIGRATOR, StartupError, connect_with_retry, verify_migrations, collation_version_warning
├── main.rs                # CHANGE: startup sequence before HttpServer; PgPool as app data; routes unchanged
└── db/shemas_definition.sql   # DELETE with `src/db/` (FR-007)
tests/
├── db/                    # one test binary "db" for all schema tests
│   ├── main.rs            # declares every module up front (empty files) so test files can be written in parallel
│   ├── support.rs         # §Test strategy helpers
│   ├── migrations.rs  schema_catalog.rs  identifiers.rs  geologic_time.rs  taxonomy.rs  geography.rs
│   ├── species.rs  collation.rs  concurrency.rs  isolation.rs  startup_checks.rs  indexes.rs
│   └── fixtures/index_dataset.sql   # data-model §9
├── startup.rs             # runs the API binary
└── repo_hygiene.rs        # std + `git` only
```

## Local database service (FR-001, FR-002)

`compose.yaml`, one service `db` (API not containerized; hosting out of scope). Start: `docker compose up -d --wait db`.

| Setting | Value | Why |
|---|---|---|
| `image` | `postgres:18-trixie` | R1 |
| `environment` | `POSTGRES_USER: ${POSTGRES_USER:?set POSTGRES_USER in .env}` (same for `PASSWORD`, `DB`); `POSTGRES_INITDB_ARGS: "--encoding=UTF8 --locale-provider=builtin --builtin-locale=C.UTF-8"` | R6, R7 |
| `command` | `postgres -c max_connections=200` | parallel test databases |
| `ports` | `"127.0.0.1:${POSTGRES_PORT:-5432}:5432"` | local only |
| `volumes` | `pgdata:/var/lib/postgresql` | R1 |
| `healthcheck` | `["CMD-SHELL", "pg_isready -U \"$$POSTGRES_USER\" -d \"$$POSTGRES_DB\""]`, interval 2s, timeout 3s, retries 15, start_period 5s | `--wait` returns when ready |

Check after start: `SELECT datlocprovider, datlocale FROM pg_database WHERE datname = current_database()` → `b | C.UTF-8`.

## Startup contract (FR-002–FR-004, AC 2)

Order: **config → connect → migration check → collation check → bind**. Each failure prints one stderr line `paleo_api: <message>` and exits **1**. No message contains the URL, user, password, host, or driver text. Warnings print `paleo_api: warning: <message>`.

| # | Step | Failure → `StartupError` variant → message |
|---|---|---|
| 1 | Read `DATABASE_URL` | unset/empty → `MissingConfig` → `DATABASE_URL is not set. Set it to a PostgreSQL URL (see .env.example).` |
| 2 | `PgConnectOptions::from_str` | `InvalidConfig` → `DATABASE_URL is not a valid PostgreSQL connection URL.` |
| 3 | `connect_with_retry(opts, 10 s)` (R5) | `Unreachable` → `Could not connect to the database within 10 s. Is it running? (docker compose up -d --wait db)` · `AuthFailed` (`28P01`/`28000`) → `The database rejected the configured credentials.` · `DatabaseMissing` (`3D000`) → `The configured database does not exist.` |
| 4 | `verify_migrations(conn, &MIGRATOR)` (R4) | `MigrationsPending { versions }` → `Database schema is out of date: N migration(s) pending (<versions>). Run "sqlx migrate run", then start the API again.` · `MigrationDirty(v)` → `Migration <v> failed partway; fix it and rerun "sqlx migrate run".` · `MigrationModified(v)` → `Migration <v> was modified after it was applied.` · unknown applied version → warning |
| 4b | `collation_version_warning(conn)`: `SELECT collversion IS DISTINCT FROM pg_collation_actual_version(oid) FROM pg_collation WHERE collname = 'paleo_name_sort'` | warning only: `Collation paleo_name_sort changed version; name-sorted pages may be misordered. REINDEX the name-sort indexes, then ALTER COLLATION paleo_name_sort REFRESH VERSION (see data-model §1.4).` |
| 5 | `PgPoolOptions::new().max_connections(10)`, `web::Data<PgPool>`, bind `127.0.0.1:8000` (unchanged) | as today |

Library signatures (public, so tests call them directly):

```rust
// src/config.rs
pub fn database_options_from_env(get: impl Fn(&str) -> Option<String>) -> Result<PgConnectOptions, StartupError>;
// src/db.rs
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
pub enum StartupError { MissingConfig, InvalidConfig, Unreachable, AuthFailed, DatabaseMissing,
                        MigrationsPending { versions: Vec<i64> }, MigrationDirty(i64), MigrationModified(i64) }
pub async fn connect_with_retry(opts: &PgConnectOptions, deadline: Duration) -> Result<PgConnection, StartupError>;
pub async fn verify_migrations(conn: &mut PgConnection, migrator: &Migrator) -> Result<Vec<String>, StartupError>; // Ok = warnings
pub async fn collation_version_warning(conn: &mut PgConnection) -> Option<String>;
```

## Enforcement matrix (spec §Integrity → mechanism)

Names are defined in data-model.md.

| Rule | Mechanism | Concurrency |
|---|---|---|
| Identifiers: unique, slug | `<t>_pk`, `<t>_id_ck` | PK unique index |
| Identifiers: immutable | `<t>_id_immutable_trg` | row-local |
| Text trimmed, non-empty, Unicode length | `!~ '^\s\|\s$'` + `char_length` CHECKs | row-local |
| Names | `<t>_name_ck`, `<t>_name_uq` on `lower(name)`; `species_scientific_name_uq` | unique index |
| Enums | `species_diet_ck`, `continents_type_ck` | row-local |
| Time ranges | `*_range_ck`, `*_start_mya_ck`, `*_end_mya_ck` on unconstrained `numeric` (R8) | row-local |
| Era / period overlaps | `eras_range_ex`, `periods_range_ex` (global, R9) | exclusion waits, then `23P01` |
| Containment | `periods_within_era_trg`, `eras_contains_periods_trg` (R10) | row locks serialize; RR guard |
| Hierarchy | NOT NULL parent FKs; `species_genus_fk` | FK |
| Geography | composite `country_continents_continent_fk` `ON UPDATE RESTRICT` + `country_continents_continent_type_ck` (R11) | FK |
| Species text | `species_description_ck`, `species_image_url_ck` | row-local |
| Species size | `species_{length,height,weight}_ck` on inline nullable pairs | row-local |
| Discovery year | `species_discovery_year_ck` + `species_discovery_year_trg` (R12) | row-local |
| List minimums | deferred `countries_min_continents_trg`, `country_continents_min_trg`, `species_min_periods_trg`, `species_periods_min_trg` (R10) | parent `FOR NO KEY UPDATE`; RR guard |
| References; links once | FKs; link-table PKs | FK / unique |
| Deletes | `ON DELETE RESTRICT` except 4 owner FKs (`CASCADE`); size inline | FK |
| TRUNCATE bypass (all rules) | `<t>_no_truncate_trg` on 16 tables (data-model §1.2) | statement-level |
| Name sorts (FR-016) | `paleo_name_sort` + `*_name_sort_idx`; version check at startup 4b | ordering only |
| Concurrency | `paleo_require_safe_isolation()` in every cross-row trigger | READ COMMITTED, SERIALIZABLE safe; RR writes → `0A000` |

## Test strategy

### Rules

- Rejected-write tests call `assert_violation(result, sqlstate, constraint)`, compare `snapshot` before/after, and run a matching valid write that succeeds (AC 3).
- Only `tests/startup.rs` and `tests/repo_hygiene.rs` mention `DATABASE_URL`; all other tests connect through the harness (R14).
- No credentialed URL literal anywhere unless the password is `change-me`. Fake-credential URLs (user `leaky_user`, password `leaky-secret-123`) are built at runtime with the scheme outside the literal: `let scheme = "postgres"; format!("{}://{}:{}@{}/{}", scheme, user, pass, host, db)`.
- A test task may add a missing function as a `todo!()` stub so the build compiles; new tests must fail for their stated reason, old tests stay green.
- Dynamic SQL is wrapped in `AssertSqlSafe` (R2).
- **Concurrency tests:** two transactions plus an observer connection; tx2's blocking statement runs in `tokio::spawn`; before tx1 ends, `wait_until_blocked(observer, tx2_pid, tx1_pid, 5 s)` is true and the handle is unfinished; every await in `tokio::time::timeout(10 s)`; `SET LOCAL lock_timeout = '8s'` per transaction.

### Helpers (`tests/db/support.rs`)

`assert_violation` (reads `DatabaseError::code()`/`constraint()`) · `snapshot(pool, table) -> Vec<String>` (`row_to_json(t)::text`, ordered by every column) · `utc_year(pool)` (DB clock, `now() AT TIME ZONE 'UTC'`) · builders `era`, `period`, `domain`…`genus`, `continent(type)`, `country_with_continent`, `species_with_period` · `explain(conn, sql, seqscan_off)` (`EXPLAIN (COSTS OFF)`, `SET LOCAL enable_seqscan = off`) · `explain_analyze(conn, sql)` (`ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF`) · `index_rows(plan, index) -> f64` (parses `Bitmap Index Scan on <index> (actual rows=N`) · `assert_uses_any_index(plan, &[..])` · `assert_no_seq_scan_on(plan, &[..])` · `backend_pid(conn)` · `wait_until_blocked(observer, blocked, blocker, timeout)` (polls `SELECT $2 = ANY(pg_blocking_pids($1))` every 50 ms) · a `SELECT 1` smoke test.

### Cases by file

| File | Covers | Cases |
|---|---|---|
| `repo_hygiene.rs` | AC 13, AC 15, FR-002, FR-007, FR-015 | (a) `.gitignore` has `.env`; (b) `.env.example` defines `POSTGRES_USER/PASSWORD/DB/PORT`, `DATABASE_URL`, `TEST_DATABASE_URL`, all passwords `change-me`, `TEST_DATABASE_URL` ends `/postgres`; (c) `git ls-files` lacks `.env`; (d) `git grep -nE 'postgres(ql)?://[^:@[:space:]]+:[^@[:space:]]+@' -- . ':!docs'` matches only `change-me`; (e) draft schema file absent; (f) `DATABASE_URL` text appears in `tests/**/*.rs` only in the two allowed files; (g) master-DB guard: harness `DATABASE_URL` (env, else `.env`) must name database `postgres` (path after last `/`, before `?`), else fail with: `Tests must run with the documented command DATABASE_URL="$TEST_DATABASE_URL" cargo test (the postgres maintenance database). This run may have written the test harness's _sqlx_test bookkeeping schema into the development database; no resource data was read or changed. Clean up with the step in quickstart §5.` |
| `db/migrations.rs` | AC 1, FR-005, FR-006, FR-013 | `migrations = false`. (a) `MIGRATOR.run` on empty DB succeeds; (b) second run adds no `_sqlx_migrations` rows and the fingerprint is identical — ordered catalog query over user `pg_class`/`pg_attribute`, `pg_constraint`, `pg_index` + `pg_get_indexdef`, `pg_trigger`, `pg_proc` in `public`, `pg_collation` in `public`, `pg_extension`; (c) `undo(0)` then `run` → same fingerprint; (d) foundation objects: `pg_trgm`, `paleo_name_sort` (`collprovider = 'i'`, deterministic, locale `und`), the three `paleo_*` functions; (e) `paleo_require_safe_isolation()` fails `0A000` under RR, succeeds under READ COMMITTED |
| `db/schema_catalog.rs` | FR-008–FR-010, FR-012 | Exactly the 16 tables of data-model §2–§5 (+ `_sqlx_migrations`); no `taxonomy`/`user`/`role`/`user_role`; no `character varying`; every `numeric` unconstrained; every constraint, integrity index, trigger, trigger function, and helper of data-model §2–§5, §6.2 exists (query-only indexes excluded); FK actions per data-model §1.5; the 4 minimum triggers are deferred constraint triggers; 16 `BEFORE TRUNCATE` statement triggers |
| `db/identifiers.rs` | AC 7, AC 8, text rules, TRUNCATE | Over all 12 resource tables: slugs `A`, `a`, `-ab`, `ab-`, `a--b`, `Ab`, 65 chars → `<t>_id_ck`, 64 chars OK; duplicate id → `23505 <t>_pk`; `SET id` → `23514 <t>_id_immutable_ck`; names `''`, `' x'`, `'x '`, `'x '`, 65 chars → `<t>_name_ck`; `'ñ'×64` OK; except species: case-variant duplicate → `23505 <t>_name_uq`, case-only self-rename OK. TRUNCATE on a fully populated chain: each resource table → `0A000` (SQLSTATE only); each link table → `23514 <t>_no_truncate_ck`; `TRUNCATE <t> CASCADE` on each resource table → `23514 <t>_no_truncate_ck`; all snapshots unchanged |
| `db/geologic_time.rs` | AC 3–6, AC 10, FR-009 | 251.902–66.0 + 66.0–0 stored; 70.0–0 → `23P01 eras_range_ex`; `start ≤ end` → `eras_range_ck`; `4600.001`, `-0.001`, `NaN`, `Infinity`, `66.0001` → `*_mya_ck`; `start_mya::text = '251.902'`; same on `periods`, plus sibling overlap → `periods_range_ex`, touching OK; NULL `era_id` → `23502`; unknown → `23503 periods_era_fk`; period outside era → `periods_within_era_ck`; era shrink → `eras_contains_periods_ck`; move to non-containing era → `periods_within_era_ck`, move with valid new range OK; delete era with periods → `23503 periods_era_fk` |
| `db/taxonomy.rs` | AC 3, AC 10 | Per rank with parent: NULL → `23502`; unknown → `23503 <rank>_<parent>_fk`; delete parent with children → `23503`, leaf delete OK; reparent to existing OK, to missing rejected; `domains` has no parent column |
| `db/geography.rs` | AC 3, AC 9, AC 10, AC 17 | `'Modern'`, `'ancient'` → `continents_type_ck`; link to prehistoric → `23503 country_continents_continent_fk`; explicit `continent_type = 'prehistoric'` rejected; linked continent → prehistoric → `23503` (unlinked change and linked rename OK); delete linked continent → `23503`; country delete removes links; duplicate link → `23505 country_continents_pk`; country without link fails at `COMMIT` (`countries_min_continents_ck`); country + first link in one tx commits; removing last link fails at `COMMIT` |
| `db/species.rs` | AC 3, AC 6, AC 8, AC 10, AC 16, AC 17 | `'CARNIVORE'` → `species_diet_ck`; same `name` twice OK; `scientific_name` case-duplicate → `23505 species_scientific_name_uq`, case-only self-rename OK; `name` 65 → `species_name_ck`; `scientific_name` `''`, `' T. rex'`, `'T. rex '`, `'T. rex '`, 129 → `species_scientific_name_ck`; description `''`, 1001, leading/trailing `\s` (incl. U+00A0) → `species_description_ck`; image_url 2049, `ftp://x`, `HTTP://x`, trailing space → `species_image_url_ck`, `https://example.org/a.png` OK; per size measure: `1.1234567`, `min > max`, `0`, negative, `NaN`, `Infinity`, one bound only → `species_<m>_ck`, `0.000001`–`1.5` and all-NULL OK; `utc_year + 1` on insert and update → `species_discovery_year_not_future_ck`, `1599` → `species_discovery_year_ck`, `1600` and `utc_year` OK; NULL/unknown genus → `23502`/`23503 species_genus_fk`; unknown link targets and unknown species → `23503` per link FK; duplicate links → `23505` per link PK; prehistoric continent link OK; minimum periods as in geography; deletes of linked period/genus/continent/country → `23503` per FK; species delete removes links |
| `db/concurrency.rs` | AC 5, AC 11, AC 17 | (a) tx1 inserts era 251.902–66.0, tx2 inserts 70.0–0, blocked; tx1 commits → tx2 `23P01`; rollback variant → tx2 succeeds; (b) same for periods of one era; (c) tx1 shrinks era, tx2 inserts period outside new range, blocked → `periods_within_era_ck`; (d) tx1 inserts period, tx2 shrinks era, blocked → `eras_contains_periods_ck`; (e) country with 2 links: tx1 deletes A + `SET CONSTRAINTS country_continents_min_trg IMMEDIATE`, tx2 deletes B and its `COMMIT` is blocked; tx1 commits → tx2 `countries_min_continents_ck`, exactly one link removed; (f) same for species periods; (g) insert into `periods` under RR → `0A000`; (h) case (c) under SERIALIZABLE → blocked, then `40001` |
| `db/isolation.rs` | AC 12, FR-014 | Two tests each insert era `shared-id` (`Shared`, 10.0–0) and see `count(*) = 1`. Passes when written (regression guard) |
| `db/startup_checks.rs` | FR-003, FR-004, FR-016 | `connect_with_retry` to `127.0.0.1:9`, 1 s deadline → `Unreachable` within 1.5 s; `verify_migrations`: no migrations → `MigrationsPending` (all versions); all applied → `Ok(vec![])`; last row deleted → `MigrationsPending { [last] }`; `success = false` → `MigrationDirty(v)`; altered `checksum` → `MigrationModified(v)`; unknown higher version → `Ok` + 1 warning; `collation_version_warning` → `None`, then `Some` after `UPDATE pg_collation SET collversion = '0' WHERE collname = 'paleo_name_sort'`; no call changes `_sqlx_migrations` |
| `src/config.rs` (unit) | AC 2, FR-002 | unset/empty → `MissingConfig`; `not a url`, `mysql://u:p@h/db` → `InvalidConfig`; `postgres://localhost/paleo` → `Ok`; every `Display` names problem and fix and contains none of `leaky_user`, `leaky-secret-123`, `localhost`, `postgres://` |
| `startup.rs` | AC 2, AC 15 | Binary with `env_clear()` + `PATH`, killed after 15 s; per-test URLs from the harness URL with the database replaced by `pool.connect_options().get_database()`. (a)–(g) exit non-zero, one `paleo_api: …` line with the gist, never `leaky_user`/`leaky-secret-123`: (a) unset; (b) `not-a-url`; (c) `leaky_user:leaky-secret-123@127.0.0.1:9/paleo` exits within 13 s; (d) right user, wrong password → credentials rejected immediately; (e) unmigrated DB → pending, `_sqlx_migrations` still absent; (f) last row deleted → `1 migration(s) pending`, rows unchanged; (g) `success = false` → `failed partway`, rows unchanged. (h) collversion `'0'` → warns and starts: `TcpListener::bind("127.0.0.1:8000")` must succeed first (else fail `port 8000 busy: stop the process using it`), then `TcpStream::connect` within 10 s, `try_wait()` is `Ok(None)`, stderr has `paleo_api: warning:` and `paleo_name_sort`; kill. Only test binding 8000 |
| `db/indexes.rs` | AC 14, FR-011, FR-012 | Fixture + `ANALYZE`. Every data-model §6.1 shape (exact text, `NAME_SORT` expanded, every sort direction, plus `count(*)` variant) with `seqscan_off`: `assert_uses_any_index(allowed)` + no Seq Scan on the 16 tables. Default planner: Q-S1 (`LIMIT 20 OFFSET 40`), Q-S13, Q-S5, Q-S7, Q-S10 have no `Seq Scan on species`. Each §6.2 lookup (e.g. `SELECT 1 FROM periods WHERE era_id = $1`) uses its index. Q-S11 count with the fixture's two terms under `explain_analyze` + `seqscan_off`: both trigram indexes appear, `index_rows` of each < 1,000, recheck removals absent or < 1,000. Punctuation-only `q` (`'%\%\%\_%'`) page (`LIMIT 20`) and count, default planner, 1 warm-up + 20 timed runs each: p95 < 50 ms. Failures print shape id, allowed set, plan |
| `db/collation.rs` | AC 18, FR-016 | Species `Zebra`, `Oviraptor`, `Ñandú`, `Nandu`, `nandu` (ids decide the tie): `ORDER BY lower(name) COLLATE paleo_name_sort, id` → `Nandu`/`nandu` by id, `Ñandú`, `Oviraptor`, `Zebra`; `DESC, id` → reverse, ties still by id; same on `eras`; `seqscan_off` plan uses `species_name_sort_idx`; `collversion` non-null and equal to `pg_collation_actual_version` |
| manual (quickstart) | AC 1, AC 13, AC 15 | §3 fresh build; §5.1 dev-data hash; §5.3 git checks |

## Draft defects (`src/db/shemas_definition.sql`, FR-007)

| # | Draft | Problem | Replacement |
|---|---|---|---|
| 1 | `VARCHAR(8)` ids, no format check | ids are 2–64-char slugs | `text` + `<t>_id_ck` |
| 2 | `name VARCHAR(8/16)` | 1–64 chars, trimmed | `text` + `<t>_name_ck` |
| 3 | `name … UNIQUE` (case-sensitive) | must ignore case | unique index on `lower(name)` |
| 4 | `start_mya`/`end_mya INT` | decimals ≤ 3 dp | `numeric` + round CHECKs |
| 5 | no range/overlap/containment rules | integrity rules | `*_range_ck`, `*_range_ex`, containment triggers |
| 6 | `period.era` nullable, `ON DELETE CASCADE` | exactly one era; delete protection | `era_id NOT NULL`, `RESTRICT` |
| 7 | ranks without parent FKs; flat `taxonomy` with `SET NULL` | hierarchy; orphaning | 7 rank tables with NOT NULL parent FKs |
| 8 | uppercase enums | lowercase | lowercase CHECKs |
| 9 | `continent_country` CASCADE both ways, no modern rule | delete protection; modern-only | `country_continents`, composite FK, CASCADE from country only |
| 10 | `specie.name UNIQUE` | not unique | no unique |
| 11 | `scientific_name VARCHAR(64)`, `description VARCHAR(512)` | 128 / 1000 | `text` + CHECKs |
| 12 | nullable `specie.taxonomy` | exactly one genus | `genus_id NOT NULL` → `genera` |
| 13 | `specie_size` `INT`, typo `max_leght_m`, CHECK on wrong column, no pair/`min ≤ max` rules | decimals ≤ 6 dp, pairs, ≥ 1 measure | inline pairs + `species_<m>_ck` |
| 14 | unchecked `discovery_year`, `image_url` | ≥ 1600, ≤ UTC year; ≤ 2048, `http(s)://` | CHECK + trigger; `species_image_url_ck` |
| 15 | no species–country table | `country_ids` | `species_countries` |
| 16 | species link FKs without delete behavior or minimums | owner cascade, target restrict, ≥ 1 period | as data-model §5.2 |
| 17 | `role`, `"user"`, `user_role` | FR-010 | removed |
| 18 | trailing `DROP TABLE …` (typo `peroid`) | destroys schema, fails | removed; down migrations |
| 19 | singular/reserved names (`"order"`, `"user"`, `specie`) | quoting hazards | plural `snake_case` |
| 20 | no indexes beyond PKs | FR-011, FR-012 | data-model §6 |

## Notes for Spec 002

1. Map errors by SQLSTATE + constraint name (data-model §8); never forward `DETAIL`/`MESSAGE`/`HINT` (`DETAIL` has row values).
2. Use READ COMMITTED; RR writes fail `0A000`. Minimum-count errors appear at **COMMIT**.
3. Era + periods in one transaction: shrink periods before the era; grow the era before the periods.
4. Name sorts must be exactly `ORDER BY lower(<col>) COLLATE paleo_name_sort [DESC], id`. The database's `lower()` is the uniqueness authority; Rust `to_lowercase` differs in rare cases, so always map `23505` to `409` even after a pre-check.
5. `species_image_url_ck` requires a lowercase scheme: lowercase it before storing, or reject uppercase.
6. `q`: escape `\`, `%`, `_`, use `ILIKE … ESCAPE '\'`, keep data-model §6.1 shapes. Decode `numeric` exactly; sizes are unbounded, so prefer `bigdecimal` (`rust_decimal` caps near 7.9e28) or reject unrepresentable values.
7. TRUNCATE is always rejected; Spec 002 tests get clean state from `#[sqlx::test]` databases.

## Risks

| Risk | Mitigation |
|---|---|
| Trigger-enforced rule has a gap | Concurrency tests in both orders; RR guard; declarative mechanisms preferred |
| Planner prefers Seq Scan on tiny tables | AC 14 method (R15) |
| Non-selective punctuation-only `q` | Accepted (spec Clarifications); `indexes.rs` asserts p95 < 50 ms |
| Tests run against the dev DB | R14: hygiene guard fails the run; AC 13 dump shows it; cleanup `DROP SCHEMA _sqlx_test CASCADE` (quickstart §5) |
| pg_dump random restrict key | `--restrict-key=paleoac13` (R14) |
| ICU drift on image change | Pinned image; startup warning 4b; runbook data-model §1.4 |
| Spec 002 omits `COLLATE paleo_name_sort` | Normative expression (data-model §1.2); Spec 002 reuses §6.1 shapes |
| Parallel tests exhaust connections | `max_connections=200`; fallback `--test-threads` |
| CRLF changes migration checksums | `.gitattributes` (R3) |
| Write at New Year's midnight | Year from DB clock at transaction start; tests use `utc_year` |

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| `pg_trgm` | FR-011 substring `q` | btree cannot serve infix `ILIKE`; full-text matches words (R13) |
| `tokio` dev-dep | AC 11/17 need two transactions in flight with timeouts | sequential awaits deadlock; `futures` is also new and has no timeout; already in tree (R14) |
| 16 `BEFORE TRUNCATE` guards | TRUNCATE bypasses minimum triggers; CASCADE bypasses RESTRICT | revoking TRUNCATE does not bind owner/superuser (the local role) |
| Containment and minimum-count triggers | cross-row rules CHECK/FK cannot express | counter columns add a write path; app-only checks break Constitution V |
| `paleo_require_safe_isolation()` | trigger checks race under RR (verified) | documenting "don't use RR" leaves the DB unprotected (US2) |
| `src/lib.rs` | tests call startup/migration functions | binary-only tests cannot use short deadlines or classify errors |
| ICU `paleo_name_sort` + separate `*_name_sort_idx` | FR-016 without uniqueness depending on ICU | ICU default or sorting via unique index ties integrity to ICU; code-point order fails AC 18 (R7) |
| Runtime catalog queries | DB-free builds until Spec 002 | `query!` needs a DB or `.sqlx` cache for two lookups |
