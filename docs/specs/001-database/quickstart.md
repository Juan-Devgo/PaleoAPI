# Quickstart & Validation — Spec 001 Database Foundation

Run every command from the repository root.

## 1. Prerequisites

| Tool | Version | Check |
|---|---|---|
| Rust toolchain | ≥ 1.94 (sqlx 0.9 MSRV), edition 2024 | `rustc --version` |
| Docker Engine + Compose v2 | any current | `docker compose version` |
| Access to the docker socket | your user is in the `docker` group | `docker ps` runs without `permission denied` |
| sqlx-cli | 0.9.x | `cargo install sqlx-cli --no-default-features --features postgres` then `sqlx --version` |

## 2. Configure the environment

```bash
cp .env.example .env          # then replace every "change-me" placeholder in .env
set -a; . ./.env; set +a      # export the variables into this shell (the API does not read .env itself)
```

Variables:

| Variable | Used by | Example (placeholder) |
|---|---|---|
| `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB` | compose `db` service | `paleo`, `change-me`, `paleo` |
| `POSTGRES_PORT` | compose host port (default `5432`) | `5432` |
| `DATABASE_URL` | API, `sqlx migrate` | `postgres://paleo:change-me@localhost:5432/paleo` |
| `TEST_DATABASE_URL` | test harness master database (the `postgres` maintenance DB, never the dev DB) | `postgres://paleo:change-me@localhost:5432/postgres` |

## 3. Start the database and build the schema (US1, AC 1)

```bash
docker compose up -d --wait db     # one command: starts PostgreSQL 18 (postgres:18-trixie) and returns once the healthcheck passes
sqlx migrate run                   # one command: applies every migration in migrations/
sqlx migrate run                   # run again: applies nothing
sqlx migrate info                  # every migration is listed as installed
```

**Expected:** the first `migrate run` applies 6 migrations with no errors. The second run applies none. No manual SQL is needed.

## 4. Run the API (US5, AC 2)

```bash
cargo run
```

**Expected:** listens on `127.0.0.1:8000`. Refusals exit non-zero with a credential-free message (plan §Startup contract):

| Scenario | How to reproduce | Expected message (gist) |
|---|---|---|
| Config missing | `env -u DATABASE_URL cargo run` | `DATABASE_URL is not set …` (immediate) |
| Database unreachable | `docker compose stop db && cargo run` | `could not connect … within 10 s …` (exits in about 10 s) |
| Migrations pending | `sqlx migrate revert && cargo run` | `… 1 migration(s) pending … run sqlx migrate run` |

Restore with `docker compose up -d --wait db && sqlx migrate run`. After a refusal, `sqlx migrate info` still shows the same pending migration.

## 5. Run the tests (US3, FR-013–FR-015)

```bash
set -a; . ./.env; set +a
DATABASE_URL="$TEST_DATABASE_URL" cargo test   # every DB test runs in its own throwaway database
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

- Failed tests leave their per-test database for debugging; the next run removes it.
- **Only the command above is supported.** If plain `cargo test` ran with the dev `DATABASE_URL`, `tests/repo_hygiene.rs` fails the run; clean the harness bookkeeping from the dev database with `docker compose exec -T db psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c 'DROP SCHEMA IF EXISTS _sqlx_test CASCADE'`.
- If tests fail with `too many clients`, run them with fewer threads (`-- --test-threads=8`). Compose already raises `max_connections` to 200.

### 5.1 AC 13 — the dev database is untouched by a test run

```bash
pg_dump_in_db() { docker compose exec -T db pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" --data-only --restrict-key=paleoac13; }
pg_dump_in_db | sha256sum > /tmp/before.sha
DATABASE_URL="$TEST_DATABASE_URL" cargo test
pg_dump_in_db | sha256sum > /tmp/after.sha
diff /tmp/before.sha /tmp/after.sha && echo "dev data unchanged"
```

Insert a few dev rows first. **Expected:** `dev data unchanged`, and `SELECT to_regnamespace('_sqlx_test') IS NULL` is true in the dev DB. Record the result in the PR description.

### 5.2 AC 14 — index usage

`DATABASE_URL="$TEST_DATABASE_URL" cargo test --test db indexes` — every shape passes.

### 5.3 AC 15 — no credentials in the repository

```bash
git ls-files | grep -xE '\.env' && echo "FAIL: .env is tracked" || echo "ok: .env not tracked"
git grep -nE 'postgres(ql)?://[^ ]*:[^@ ]+@' -- . ':!docs' | grep -v 'change-me' && echo "FAIL" || echo "ok: only placeholders"
```



### 5.4 AC 18 — linguistic name sort

`DATABASE_URL="$TEST_DATABASE_URL" cargo test --test db collation`. By hand:

```bash
docker compose exec -T db psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c \
  "SELECT n FROM (VALUES ('Zebra'),('Oviraptor'),('Ñandú'),('Nandu')) v(n) ORDER BY lower(n) COLLATE paleo_name_sort"
```

**Expected:** `Nandu`, `Ñandú`, `Oviraptor`, `Zebra`.

## 6. Collation version check (FR-016, ops)

After any image base change:

```bash
docker compose exec -T db psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc \
  "SELECT collversion, pg_collation_actual_version(oid) FROM pg_collation WHERE collname = 'paleo_name_sort'"
```

**Expected:** equal values (e.g. `153.128|153.128`). Otherwise repair per data-model §1.4.

## 7. Reset

```bash
docker compose down -v      # removes the container and the data volume
```
