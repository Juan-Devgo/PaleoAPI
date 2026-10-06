# Tasks: Spec 001 — Database Foundation

## Phase 1: Setup

- [X] T001 Add `rust-version`, `sqlx` 0.9 and dev-only `tokio` (plan §Technical Context) in `Cargo.toml`
- [X] T002 [P] Pin LF endings for migrations in `.gitattributes`
- [X] T003 [P] Add migrations rerun trigger in `build.rs`

## Phase 2: Foundational

- [X] T004 Write hygiene tests (a)–(e) (plan §Test strategy) in `tests/repo_hygiene.rs`
- [X] T005 Ignore `.env` in `.gitignore`
- [X] T006 Create placeholder env file (quickstart §2) in `.env.example`
- [X] T007 Delete draft schema and `src/db/` in `src/db/shemas_definition.sql`
- [X] T008 Create `db` service (plan §Local database service) in `compose.yaml`
- [X] T009 Create library skeleton, `MIGRATOR`, and empty `foundation` migration in `src/lib.rs`, `src/config.rs`, `src/db.rs`, `migrations/`
- [X] T010 Create test harness, helpers, and empty module files (plan §Test strategy) in `tests/db/main.rs`, `tests/db/support.rs`
- [X] T011 Write migration build, idempotence, and reversibility tests in `tests/db/migrations.rs`
- [X] T012 Implement `foundation` migration (data-model §1.3) in `migrations/<ts>_foundation.{up,down}.sql`

## Phase 3: US2 — Integrity enforced by the database (P1, MVP)

- [X] T013 [P] [US2] Write schema catalog tests in `tests/db/schema_catalog.rs`
- [X] T014 [P] [US2] Write geologic time tests in `tests/db/geologic_time.rs`
- [X] T015 [P] [US2] Write taxonomy tests in `tests/db/taxonomy.rs`
- [X] T016 [P] [US2] Write geography tests in `tests/db/geography.rs`
- [X] T017 [P] [US2] Write species tests in `tests/db/species.rs`
- [X] T018 [P] [US2] Write identifier, name, and TRUNCATE guard tests in `tests/db/identifiers.rs`
- [X] T019 [P] [US2] Write concurrent write tests in `tests/db/concurrency.rs`
- [X] T020 [US2] Implement `geologic_time` migration (data-model §2) in `migrations/<ts>_geologic_time.{up,down}.sql`
- [X] T021 [US2] Implement `taxonomy` migration (data-model §4) in `migrations/<ts>_taxonomy.{up,down}.sql`
- [X] T022 [US2] Implement `geography` migration (data-model §3) in `migrations/<ts>_geography.{up,down}.sql`
- [X] T023 [US2] Implement `species` migration (data-model §5) in `migrations/<ts>_species.{up,down}.sql`

## Phase 4: US1 — One-command database and schema (P1)

- [X] T024 [US1] Update §3 Commands (plan §Project Structure) in `AGENTS.md`
- [X] T025 [P] [US1] Write Getting started in `README.md`
- [X] T026 [US1] Validate AC 1 on a fresh clone per quickstart §2–§3; fix doc drift in `README.md`, `AGENTS.md`

## Phase 5: US3 — Isolated tests, dev data untouched (P1)

- [X] T027 [P] [US3] Write parallel same-id isolation tests in `tests/db/isolation.rs`
- [X] T028 [P] [US3] Add hygiene guards (f) source scan and (g) master database in `tests/repo_hygiene.rs`
- [X] T029 [US3] Validate AC 13 per quickstart §5.1; record result in the PR description

## Phase 6: US5 — Environment-only config and safe startup (P2)

- [X] T030 [P] [US5] Write config unit tests with `StartupError` and `database_options_from_env` stubs in `src/config.rs`, `src/db.rs`
- [X] T031 [US5] Implement `database_options_from_env` and `StartupError` `Display` in `src/config.rs`, `src/db.rs`
- [X] T032 [US5] Write startup check tests with `connect_with_retry`, `verify_migrations`, `collation_version_warning` stubs in `tests/db/startup_checks.rs`, `src/db.rs`
- [X] T033 [P] [US5] Write binary startup tests (a)–(h) in `tests/startup.rs`
- [X] T034 [US5] Implement `connect_with_retry`, `verify_migrations`, `collation_version_warning` (research R4, R5) in `src/db.rs`
- [X] T035 [US5] Implement startup sequence and pool registration (plan §Startup contract) in `src/main.rs`

## Phase 7: US4 — Index-backed queries and linguistic sorts (P2)

- [X] T036 [P] [US4] Create 10,000-species dataset (data-model §9) in `tests/db/fixtures/index_dataset.sql`
- [X] T037 [P] [US4] Write planner-verified index tests in `tests/db/indexes.rs`
- [X] T038 [P] [US4] Write linguistic name sort tests in `tests/db/collation.rs`
- [X] T039 [US4] Implement `query_indexes` migration (data-model §7) in `migrations/<ts>_query_indexes.{up,down}.sql`

## Phase 8: Polish

- [X] T040 Final validation: quickstart §2–§7 after `docker compose down -v`, full gate run twice, quickstart §5.1 and §5.3, clean `git status`
