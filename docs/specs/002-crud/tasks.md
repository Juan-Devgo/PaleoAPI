# Tasks: Spec 002 — Core CRUD

## Phase 1: Setup

- [X] T001 Add `bigdecimal`, `serde_json` `arbitrary_precision`, and sqlx `bigdecimal` feature (plan §Technical Context) in `Cargo.toml`
- [X] T002 [P] Write guards (plan §Cases `repo_hygiene.rs`) in `tests/repo_hygiene.rs`
- [X] T003 Add `SQLX_OFFLINE=true` with comment (research R3) in `.env.example`
- [X] T004 [P] Add `cargo sqlx prepare -- --all-targets` to §3 and the `--check` gate to §5 in `AGENTS.md`

## Phase 2: Foundational

- [X] T005 Create `api` module tree with `todo!()` stubs for the public surface (plan §Project Structure) in `src/lib.rs`, `src/api/`
- [X] T006 Create test binary with every module declared as an empty file, plus helpers and token constants (plan §Helpers, §Rules) in `tests/api/main.rs`, `tests/api/support.rs`, `tests/api/tokens.rs`
- [X] T007 [P] Write `Decimal` unit tests (plan §Cases `decimal.rs`) in `src/api/decimal.rs`
- [X] T008 [P] Write input unit tests (plan §Cases `input.rs`) in `src/api/input.rs`
- [X] T009 [P] Write query-parameter unit tests (plan §Cases `params.rs`) in `src/api/params.rs`
- [X] T010 [P] Write content-type and `If-None-Match` unit tests (plan §Cases `http.rs`) in `src/api/http.rs`
- [X] T011 [P] Write `DenyAll` unit tests (plan §Cases `auth.rs`) in `src/api/auth.rs`
- [X] T012 [P] Write router-level tests (plan §Cases `global.rs` router) in `tests/api/global.rs`
- [X] T013 [P] Write case (h) serve (plan §Cases `startup.rs` (h) serve) in `tests/startup.rs`
- [X] T014 Implement `ApiError`, codes (spec §4.12), `FieldError`, envelope rendering, `no-store` on `GET` errors in `src/api/error.rs`
- [X] T015 Implement `Decimal` (research R1) in `src/api/decimal.rs`
- [X] T016 Implement query-string parser (plan §Read path) in `src/api/params.rs`
- [X] T017 Implement `AdminGate` and `DenyAll` (research R4) in `src/api/auth.rs`
- [X] T018 Implement write-body reader (plan §Request pipeline step 3, research R5), cached `GET` responder (research R7), envelopes, `Location` in `src/api/http.rs`
- [X] T019 [P] Implement `api_connect_options` (research R3) in `src/db.rs`
- [X] T020 Implement `Field<T>`, object walker, shared validators (research R2, R10) in `src/api/input.rs`
- [X] T021 Implement `app()`, CORS `DefaultHeaders` and `OPTIONS` short-circuit (research R6), default `404`, empty `ROUTES` (plan §Request pipeline) in `src/api/mod.rs`
- [X] T022 Replace stub handlers and `/` route with `api::app`, `api_connect_options` pool, `DenyAll` gate in `src/main.rs`

## Phase 3: US1 + US3 — Paginated species cards (MVP)

- [X] T023 [P] [US1] Write species read tests (plan §Cases `species.rs` reads) in `tests/api/species.rs`
- [X] T024 [P] [US3] Write list, caching, and `405` tests (plan §Cases `global.rs` reads) in `tests/api/global.rs`
- [X] T025 [P] [US1] Implement `SpeciesCard`, batched Q-S12 embeddings and their `*_SQL` constants (plan §Read path Embedding, §Project Structure; data-model §1; research R8), then run `cargo sqlx prepare -- --all-targets`, in `src/api/species/card.rs`, `.sqlx/`
- [X] T026 [P] [US3] Implement sorts Q-S1…Q-S3, `push_page_sql`, `push_count_sql`, count-first paging (plan §Read path) in `src/api/species/list.rs`
- [X] T027 [US1] Implement `GET /species` and `GET /species/{species_id}` (Q-S13 with `*_SQL` constant), then run `cargo sqlx prepare -- --all-targets`, in `src/api/species/mod.rs`, `.sqlx/`
- [X] T028 [US1] Register species read routes, `ROUTES` entries, and the `405` `ErrorHandlers` rewrite (plan §Request pipeline, research R5) in `src/api/mod.rs`
- [X] T029 [US1] Check query metadata with `cargo sqlx prepare --check -- --all-targets` and commit `.sqlx/`

## Phase 4: US2 — Species filters and search

- [X] T030 [US2] Write filter and search tests (plan §Cases `species_filters.rs`) in `tests/api/species_filters.rs`
- [X] T031 [US2] Implement filters Q-S4…Q-S11 and `q` escaping (plan §Read path) in `src/api/species/list.rs`
- [X] T032 [US2] Wire filter parsing into `GET /species` in `src/api/species/mod.rs`

## Phase 5: US4 — Browse timeline, taxonomy, and geography

- [X] T033 [P] [US4] Write era/period read tests (plan §Cases `geologic_time.rs` reads) in `tests/api/geologic_time.rs`
- [X] T034 [P] [US4] Write taxonomy read tests (plan §Cases `taxonomy.rs` reads) in `tests/api/taxonomy.rs`
- [X] T035 [P] [US4] Write continent/country read tests (plan §Cases `geography.rs` reads) in `tests/api/geography.rs`
- [X] T036 [P] [US4] Implement era and period reads (Q-E1…Q-E3, Q-P1…Q-P4 with joined `era`, `*_SQL` constants; plan §Read path Embedding; research R8), then run `cargo sqlx prepare -- --all-targets`, in `src/api/geologic_time.rs`, `.sqlx/`
- [X] T037 [P] [US4] Implement `Rank` descriptors, per-rank SQL macro (research R3), reads (Q-T1…Q-T3 with joined parent; plan §Read path Embedding) in `src/api/taxonomy.rs`
- [X] T038 [P] [US4] Implement continent and country reads (Q-C1…Q-C3, Q-K1, Q-K2 batched per page, `*_SQL` constants; plan §Read path Embedding), then run `cargo sqlx prepare -- --all-targets`, in `src/api/geography.rs`, `.sqlx/`
- [X] T039 [US4] Register read routes (taxonomy: six direct nested paths only) and `ROUTES` entries in `src/api/mod.rs`
- [X] T040 [US4] Check query metadata with `cargo sqlx prepare --check -- --all-targets` and commit `.sqlx/`

## Phase 6: US6 + US5 — Admin writes and actionable errors

- [X] T041 [P] [US6] Write write-contract tests (plan §Cases `global.rs` writes) in `tests/api/global.rs`
- [X] T042 [US5] Write leak and dispatch tests (plan §Cases `global.rs` leaks) in `tests/api/global.rs`
- [X] T043 [P] [US6] Write era/period write tests (plan §Cases `geologic_time.rs` writes) in `tests/api/geologic_time.rs`
- [X] T044 [P] [US6] Write rank write tests (plan §Cases `taxonomy.rs` writes) in `tests/api/taxonomy.rs`
- [X] T045 [P] [US6] Write continent/country write tests (plan §Cases `geography.rs` writes) in `tests/api/geography.rs`
- [X] T046 [P] [US6] Write species write tests (plan §Cases `species.rs` writes) in `tests/api/species.rs`
- [X] T047 [P] [US6] Write error-map completeness test (plan §Cases `error_map.rs`) in `tests/api/error_map.rs`
- [X] T048 [P] [US6] Write case (h) deny (plan §Cases `startup.rs` (h) deny) in `tests/startup.rs`
- [X] T065 [P] [US6] Write range-merge and attribution unit tests (plan §Cases `geologic_time.rs` (unit)) in `src/api/geologic_time.rs`
- [X] T066 [P] [US6] Write `size` validator unit tests (plan §Cases `species/input.rs` (unit)) in `src/api/species/input.rs`
- [X] T049 [US6] Implement SQLSTATE + constraint map, `is_mapped`, dependents lookup and message (data-model §4, §5), `500` stderr line (plan §Write path), then run `cargo sqlx prepare -- --all-targets`, in `src/api/db_error.rs`, `.sqlx/`
- [X] T050 [P] [US6] Implement era and period writes (plan §Write path; data-model §2, §3, §6), then run `cargo sqlx prepare -- --all-targets`, in `src/api/geologic_time.rs`, `.sqlx/`
- [X] T051 [P] [US6] Implement rank writes: `POST /domains`, nested create, reparent, delete (plan §Write path), then run `cargo sqlx prepare -- --all-targets`, in `src/api/taxonomy.rs`, `.sqlx/`
- [X] T052 [P] [US6] Implement continent and country writes (plan §Write path; data-model §2, §6), then run `cargo sqlx prepare -- --all-targets`, in `src/api/geography.rs`, `.sqlx/`
- [ ] T053 [P] [US6] Implement species create/patch validation incl. `size` (spec §5.6, data-model §1, research R1, R2, R10), then run `cargo sqlx prepare -- --all-targets`, in `src/api/species/input.rs`, `.sqlx/`
- [ ] T054 [US6] Implement species `POST`, `PATCH`, `DELETE` (plan §Write path), then run `cargo sqlx prepare -- --all-targets`, in `src/api/species/mod.rs`, `.sqlx/`
- [ ] T055 [US6] Register write routes and `ROUTES` methods in `src/api/mod.rs`
- [ ] T056 [US6] Check query metadata with `cargo sqlx prepare --check -- --all-targets` and commit `.sqlx/`

## Phase 7: Index and latency gate

- [ ] T057 [P] Write query-plan tests (plan §Cases `query_plans.rs`) in `tests/api/query_plans.rs`
- [ ] T058 [P] Write latency tests (plan §Cases `performance.rs`) in `tests/api/performance.rs`
- [ ] T059 Fix any non-index-served or over-budget shape in `src/api/species/list.rs`, `src/api/species/card.rs`, `src/api/geologic_time.rs`, `src/api/taxonomy.rs`, `src/api/geography.rs`

## Phase 8: Polish

- [ ] T060 Write OpenAPI drift test (plan §Cases `openapi.rs`) in `tests/api/openapi.rs`
- [ ] T061 Publish `contracts/openapi.yaml`, reconciled with `ROUTES`, as `docs/openapi.yaml`
- [ ] T062 Run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`; fix findings in `src/`, `tests/`
- [ ] T063 Run `cargo sqlx prepare --check -- --all-targets` and full test suite (quickstart §3); commit `.sqlx/`
- [ ] T064 Final validation: quickstart §1–§2 manual scenarios; fix doc drift in `docs/specs/002-crud/quickstart.md`; record `performance` p95 per shape in the PR description
