# Research — Spec 001

**Date:** 2026-10-03. "Verified" = run on PostgreSQL 18.6 (throwaway builtin `C.UTF-8` cluster, or a throwaway `postgres:18-trixie` container for ICU items).

## R1. PostgreSQL version and image

- **Decision:** PostgreSQL 18, image `postgres:18-trixie` (tracks 18.x, pins the Debian 13 base). Mount the volume at `/var/lib/postgresql` (PG 18 image sets `PGDATA=/var/lib/postgresql/18/docker`).
- **Why:** newest GA major; has everything this plan needs (exclusion constraints on `numrange`, deferrable constraint triggers, trusted `pg_trgm`, builtin `C.UTF-8` provider). Image verified: built `--with-icu`, ICU 76, `pg_trgm` in contrib.
- **Rejected:** 17 (no advantage); 19 (beta 4 on 2026-09-24, not GA); `postgres:18.6` (blocks security minors); bare `postgres:18` (a new Debian base can bring a new ICU major and change name order, R7).

## R2. Data access and migration tooling

- **Decision:** `sqlx` 0.9.0 (MSRV 1.94), `default-features = false`, features `runtime-tokio`, `postgres`, `macros`, `migrate`. `sqlx-cli` 0.9 via `cargo install sqlx-cli --no-default-features --features postgres`.
- **Why:** newest stable; `macros` gives `#[sqlx::test]`/`migrate!`, `migrate` gives the `Migrate` trait used by the startup check. No TLS: local DB has none and hosting is out of scope.
- **0.9 notes:** `query*()` takes `impl SqlSafeStr`; dynamic SQL needs `AssertSqlSafe(..)`. `cargo install --locked sqlx-cli` no longer works.
- **Rejected:** sqlx 0.8.6 (Spec 002 would have to upgrade).

## R3. Migration layout

- **Decision:** reversible pairs with timestamp versions (`sqlx migrate add -r <name>`): five domain migrations plus a final `query_indexes` (data-model §7). `.gitattributes` forces LF on `migrations/*.sql`.
- **Why:** timestamps avoid clashes across parallel worktrees; down migrations allow resets and are tested (up → down → up); a separate query-index migration lets index tests fail first; CRLF checkouts would change sqlx checksums and trip the "modified" startup check. Rerunning applies nothing (`_sqlx_migrations`).
- **Rejected:** `--sequential` (branch clashes); one big migration (hard to review); irreversible migrations (reset only by dropping the DB).

## R4. Read-only pending-migration check (FR-004)

- **Decision:** embed migrations with `sqlx::migrate!("./migrations")` for comparison only. After connecting: (1) `SELECT to_regclass('public._sqlx_migrations')`, `NULL` → all pending; (2) `Migrate::dirty_version()`, `Some(v)` → dirty; (3) `Migrate::list_applied_migrations()` vs embedded: missing → pending, checksum differs → modified; (4) applied version unknown to this build → warning only (expand/contract deploys). Never call `Migrator::run`, `ensure_migrations_table`, or `lock` (all write).
- **Rejected:** API applies migrations (clarification 2026-10-02); count-only comparison (misses gaps and edits); `sqlx migrate info` subprocess (needs CLI in production).

## R5. Bounded startup wait (FR-003)

- **Decision:** retry every 500 ms, 10 s total, each attempt capped by remaining time (`actix_web::rt::time::timeout`). `28P01`/`28000`/`3D000` fail at once. 10 s is a code constant.
- **Why:** `compose up --wait` makes the DB ready beforehand; the wait only covers restarts. Keeps the unreachable test short. No extra env var (YAGNI).
- **Rejected:** wait forever (breaks FR-003); single attempt (fragile after `compose up`); configurable timeout (not needed yet).

## R6. Configuration and secrets (FR-002)

- **Decision:** the API reads only `DATABASE_URL` and does not load `.env`; contributors export it (`set -a; . ./.env; set +a`). Compose reads `POSTGRES_USER`/`PASSWORD`/`DB` with `${VAR:?msg}` and `POSTGRES_PORT` (default 5432). `.env.example` committed with placeholders; `.env` git-ignored. `sqlx-cli` and the test harness read `.env` themselves (internal `dotenvy`).
- **Why:** env is the only config source in every environment; no new dependency. Startup errors map each failure category to a fixed message and never echo the URL or driver text (driver text can include user and host).
- **Rejected:** `dotenvy` in the API (new dependency, same effect); one variable per connection part (sqlx tooling expects `DATABASE_URL`).

## R7. Locale, case-insensitivity, linguistic sorts (FR-016, AC 18)

- **Decision:** two layers.
  1. Cluster default builtin `C.UTF-8` (initdb args in plan §Local database service). `lower()`, `\s`, `char_length`, case-insensitive uniqueness, and `pg_trgm` use it. The `foundation` migration fails unless encoding is UTF8.
  2. Name sorts use collation `paleo_name_sort` (`provider = icu, locale = 'und'`, deterministic), always as the exact expression in data-model §1.2, served by an index on the identical expression.
- **Why (verified):**
  - Builtin provider has no OS/library dependency, so integrity rules (uniqueness, trimming, lengths) never shift on upgrades. `lower('ÉCOLE Ñandú')` = `école ñandú`; `\s` matches U+00A0, U+0085, U+3000 (same as Rust `char::is_whitespace`).
  - ICU root gives `apple < Apricot < Ébano < Echo < Nandu < Ñandú < nandu-x < Oviraptor < Zebra`; code-point order puts `Ébano`/`Ñandú` after `Zebra`.
  - Sorting `lower(col)` keeps Spec 002's tie rule: case-only differences compare equal, `id` breaks ties.
  - Deterministic collation → ICU affects ordering only, never equality or uniqueness.
  - ICU ships in the image, so host OS upgrades cannot change order (FR-016); `-trixie` pins ICU 76.
  - `Mesozoic` → `MESOZOIC` rename passes the `lower(name)` unique index; a second `MESOZOIC` fails (also with the ICU sort index present).
  - Planner (20,000 rows): `ORDER BY lower(name) COLLATE paleo_name_sort, id LIMIT 20` → Index Scan; `DESC` → Index Scan Backward + Incremental Sort on `id`; without `COLLATE` the index is not used.
  - `pg_collation.collversion` tracks ICU (`153.128` for ICU 76); drift handling in data-model §1.4.
- **Rejected:** ICU as cluster default (all text indexes, uniqueness included, would depend on ICU); predefined `"und-x-icu"` (name depends on initdb import); nondeterministic `und-u-ks-level2` (uniqueness depends on ICU, `LIKE` restrictions, unpredictable ties); `C.UTF-8` order (fails FR-016); libc `en_US.utf8` (glibc changes silently corrupt indexes); `citext` (extra extension, odd casts).

## R8. Exact decimals (FR-009)

- **Decision:** unconstrained `numeric` + `CHECK (v = round(v, n))` (n = 3 for mya, 6 for sizes); size bounds also `v < 'Infinity'`.
- **Why (verified):** `numeric(p,s)` silently rounds; `numeric` accepts `NaN`/`±Infinity` since PG 14. NaN sorts above Infinity, so `< 'Infinity'` rejects both; mya `BETWEEN 0 AND 4600` already does. `300.0001`, `1.1234567`, `NaN`, `Infinity` rejected; `0.000001` accepted.
- **Rejected:** `numeric(7,3)` (rounds); `double precision` (approximate); integer thousandths (unit differs from spec).

## R9. Overlaps

- **Decision:** `EXCLUDE USING gist (numrange(end_mya, start_mya, '[)') WITH &&)` on `eras` and globally on `periods`.
- **Why (verified):** half-open ranges allow touching boundaries and reject overlaps; a concurrent inserter waits, then gets `23P01`, under any isolation. Periods sit inside non-overlapping eras, so global = per-era, without `btree_gist`. `*_range_ck` runs first, so an inverted range gives a clear error, not a `numrange` error.
- **Rejected:** per-era exclusion (needs `btree_gist`); trigger overlap checks (racy without explicit locks).

## R10. Cross-row rules under concurrency

- **Containment (verified both orders, and SERIALIZABLE → `40001`):** immediate `AFTER` row triggers; the period trigger takes the era `FOR SHARE`, which conflicts with the era update's `FOR NO KEY UPDATE`, so writers serialize and the second sees the first's committed result under READ COMMITTED.
- **Minimum counts (verified):** `DEFERRABLE INITIALLY DEFERRED` constraint triggers on parent insert and link delete/update; lock parent `FOR NO KEY UPDATE`, skip if parent is gone (cascade), raise if no link left. Of two transactions removing the last two links, the later commit fails. Deferral lets a parent and its first link share a transaction.
- **REPEATABLE READ guard (verified necessary):** under RR a trigger's check uses the old snapshot, and locking a row another transaction only locked raises no serialization error; with the guard off, a shrink/insert race committed a period outside its era. Every cross-row trigger calls `paleo_require_safe_isolation()` (`0A000` under RR). READ COMMITTED and SERIALIZABLE are safe.
- **TRUNCATE guard (verified):** TRUNCATE fires no row triggers (`TRUNCATE country_continents` leaves countries without continents) and `TRUNCATE … CASCADE` bypasses `ON DELETE RESTRICT`. A plain TRUNCATE of an FK-referenced table fails with `0A000` before any trigger, but CASCADE silently empties children. Hence a statement-level guard on all 16 tables (outcomes in data-model §1.2). It stays on resource tables to cover CASCADE and future unreferenced tables. Nothing needs TRUNCATE: tests get fresh databases; resets use `compose down -v` or down migrations.
- **Rejected:** counter columns (second write path); SERIALIZABLE everywhere (pushes retries to Spec 002, DB still unprotected); advisory locks (every writer must remember).

## R11. Modern-only country continents

- **Decision:** `country_continents.continent_type` (default and CHECK `'modern'`) + composite FK `(continent_id, continent_type) → continents(id, type)` `ON UPDATE RESTRICT`, backed by `UNIQUE (id, type)`.
- **Why (verified):** prehistoric link fails the FK; changing a linked continent's `type` changes the referenced key, so RESTRICT blocks it; rename still allowed. No trigger; any isolation.

## R12. Discovery year upper bound

- **Decision:** `CHECK (discovery_year >= 1600)` + `BEFORE INSERT OR UPDATE OF discovery_year` trigger rejecting `> extract(year FROM now() AT TIME ZONE 'UTC')`.
- **Why:** PostgreSQL docs require CHECK expressions to be immutable and warn against current time in them (dump/restore re-checks). `now()` = transaction start on the DB clock.

## R13. Substring search (`q`) — `pg_trgm`

- **Decision:** `CREATE EXTENSION IF NOT EXISTS pg_trgm`; GIN `gin_trgm_ops` on `species.name` and `species.scientific_name`; `name ILIKE $p OR scientific_name ILIKE $p` → BitmapOr.
- **Why (verified 2026-10-03, 10,000 rows):** btree cannot serve infix `ILIKE`. pg_trgm narrows only when the term has a full trigram (≥ 3 consecutive alphanumerics). `'%ab1%'` → 25 + 12 rows from the two Bitmap Index Scans, no recheck removals. `'%ab%'` → each scan returns all 10,000 rows, recheck removes 9,397 (a disguised full scan); this led to Spec 002 `q` ≥ 3. Terms like `%%_` or `a-b` still yield no trigram: correct results, ≈ 5 ms on 10,000 rows (accepted, spec Clarifications). Index presence in a plan is not proof of filtering, so AC 14 checks row counts (R15). Trusted contrib extension: no superuser, no service, no Rust dependency.
- **Collation independence (verified):** trigram indexes are on plain `C.UTF-8` columns; `ILIKE '%AND%'` matched `Nandu`, `nandu-x`, `Ñandú` with the ICU sort index present.
- **Rejected:** Seq Scan (violates FR-011); full-text search (words, not substrings: `sau` misses `Tyrannosaurus`); hand-made n-gram table.

## R14. Test isolation and dev-data separation (FR-013–FR-015)

- **Decision:** `#[sqlx::test]` (fresh `_sqlx_test_*` database per test, all migrations applied, dropped on pass). Harness master = `postgres` maintenance DB via the documented command `DATABASE_URL="$TEST_DATABASE_URL" cargo test`.
- **Why:** the harness reads only `DATABASE_URL` and creates a `_sqlx_test` bookkeeping schema in the master DB under an advisory lock; pointing it at `postgres` keeps that out of the dev DB. Misuse (plain `cargo test` with the dev URL) writes only bookkeeping there, never resource data. It is handled by detection (repo_hygiene guard fails the run; it cannot prevent the write, since the harness connects before test code runs), measurement (AC 13 dump excludes no schema), and documentation (only the `TEST_DATABASE_URL` form is shown).
- **pg_dump (verified):** 18.6 writes `\restrict`/`\unrestrict` with a random key per run; `--restrict-key=paleoac13` makes dumps deterministic.
- **Concurrency proof:** sequential execution would pass without locking, so tests assert via `pg_blocking_pids` that tx2 waits on tx1. `tokio` (`spawn`, `timeout`) is dev-only and already in the tree (1.53.1 via actix-rt/sqlx).
- **Rejected:** testcontainers (dependency; compose already provides Docker); per-test rollback (cannot test deferred triggers or concurrent commits); `futures::join!` (new dependency, no timeout).

## R15. Index verification method (AC 14)

- **Decision:** deterministic `generate_series` fixture (data-model §9), `ANALYZE`, then `EXPLAIN (COSTS OFF)` per shape (data-model §6.1) under `SET LOCAL enable_seqscan = off`: the plan must use an allowed index and have no `Seq Scan` on any resource or link table. Selective large-table shapes are also checked with default settings. `q` adds an `EXPLAIN (ANALYZE)` row-count check (R13). Assertions match `EXPLAIN` text, so no JSON dependency.
- **Why:** on tiny tables the planner rightly picks Seq Scan; `enable_seqscan = off` is the standard way to prove an index *can* serve a query (clarification 2026-10-03), and falls back to Seq Scan when none fits. Verified on 10,000 species: name page uses the (ICU) sort index; `DESC` uses Index Scan Backward + Incremental Sort; `discovery_year DESC NULLS LAST` uses its own index; `q` count uses BitmapOr over both trigram indexes.

## R16. Local environment

Docker compose is the only supported way to run the database and tests; no local-cluster fallback is documented.

## Sources

- sqlx crate metadata: https://crates.io/api/v1/crates/sqlx
- SQLx 0.9.0 announcement: https://github.com/launchbadge/sqlx/discussions/4271
- SQLx CHANGELOG: https://raw.githubusercontent.com/transact-rs/sqlx/main/CHANGELOG.md
- sqlx 0.9.0 docs: https://docs.rs/crate/sqlx/latest
- `#[sqlx::test]`: https://docs.rs/sqlx/latest/sqlx/attr.test.html
- `Migrate` trait: https://docs.rs/sqlx/latest/sqlx/migrate/trait.Migrate.html
- sqlx Postgres test harness: https://raw.githubusercontent.com/transact-rs/sqlx/main/sqlx-postgres/src/testing/mod.rs
- sqlx-cli README: https://raw.githubusercontent.com/transact-rs/sqlx/main/sqlx-cli/README.md
- PostgreSQL version history: https://versionlog.com/postgresql/
- PostgreSQL 19 Beta 4: https://www.postgresql.org/about/news/postgresql-19-beta-4-3386/
- Official postgres image (PG 18 `PGDATA`, `18-trixie` tag): https://hub.docker.com/_/postgres
- PG 18 docs: [constraints](https://www.postgresql.org/docs/18/ddl-constraints.html), [isolation](https://www.postgresql.org/docs/18/transaction-iso.html), [explicit locking](https://www.postgresql.org/docs/18/explicit-locking.html), [CREATE TRIGGER](https://www.postgresql.org/docs/18/sql-createtrigger.html), [pg_trgm](https://www.postgresql.org/docs/18/pgtrgm.html), [locale](https://www.postgresql.org/docs/18/locale.html), [collation](https://www.postgresql.org/docs/18/collation.html), [CREATE COLLATION](https://www.postgresql.org/docs/18/sql-createcollation.html), [ALTER COLLATION](https://www.postgresql.org/docs/18/sql-altercollation.html), [admin functions](https://www.postgresql.org/docs/18/functions-admin.html)
