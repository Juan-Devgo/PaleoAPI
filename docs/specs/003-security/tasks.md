# Tasks: Spec 003 — Security

## Phase 1: Setup

- [X] T001 Set dependencies, `default-run`, and dev profiles (plan §Project Structure `Cargo.toml`; research R6, R8) in `Cargo.toml`, `Cargo.lock`
- [X] T002 [P] Add the RUSTSEC-2023-0071 ignore with a comment pointing at plan §Supply chain in `.cargo/audit.toml`
- [X] T003 [P] Add the `JWT_SECRET` placeholder and commented limit variables (plan §Configuration) in `.env.example`
- [X] T004 [P] Add the vulnerable lockfile fixture (plan §Project Structure) in `tests/fixtures/audit/Cargo.lock`
- [X] T005 [P] Add pointer-only notes to §4.4, §4.11, §4.12, §9 (plan §Spec 002 overrides) in `docs/specs/002-crud/spec.md`
- [X] T006 [P] Write repository guards scoped to tracked files outside `docs/` and `tests/repo_hygiene.rs` (plan §Cases `tests/repo_hygiene.rs`) in `tests/repo_hygiene.rs`
- [X] T093 Disable actix-web's `http2` default feature and verify `h2` is absent and `cargo audit` passes (plan §Supply chain) in `Cargo.toml`, `Cargo.lock`

## Phase 2: Foundational

- [X] T007 [P] Write account table constraint and trigger tests (plan §Cases `tests/db/accounts.rs`) in `tests/db/accounts.rs`, `tests/db/main.rs`
- [X] T008 [P] Update catalog and migration tests for `accounts` (plan §Existing tests) in `tests/db/support.rs`, `tests/db/schema_catalog.rs`, `tests/db/migrations.rs`
- [X] T009 [P] Exclude `accounts` from the error-map completeness test (plan §Existing tests) in `tests/api/error_map.rs`
- [X] T010 Create the `accounts` migration with its reversible down (data-model §1) in `migrations/<ts>_accounts.up.sql`, `migrations/<ts>_accounts.down.sql`
- [X] T011 Create the `security` module tree with `todo!()` stubs (plan §Project Structure) in `src/lib.rs`, `src/security/`
- [X] T012 [P] Write configuration unit tests (plan §Cases `src/config.rs`) in `src/config.rs`
- [X] T013 [P] Write client resolution unit tests (plan §Cases `src/security/*`; research R5) in `src/security/client_ip.rs`
- [X] T014 [P] Write password policy and hashing unit tests (research R8, R9) in `src/security/password.rs`
- [X] T015 [P] Write claims round-trip unit tests (data-model §2; research R6) in `src/security/token.rs`
- [X] T016 [P] Write RFC 3339 formatter and `path` / `client` control-character escaping unit tests (plan §Cases `src/security/*`; research R17) in `src/security/events.rs`
- [X] T017 [P] Write permit unit tests (data-model §3.4) in `src/security/permits.rs`
- [X] T018 Write login and write-check lookup tests (plan §Cases `tests/db/accounts.rs`) in `tests/db/accounts.rs`
- [X] T019 [P] Write `Security` construction unit tests (plan §Cases `src/security/*`) in `src/security/mod.rs`
- [X] T020 Implement `security_config_from_env`, the new `StartupError` variants, and the `DB_TIMEOUT` constant (plan §Configuration, §Startup contract) in `src/config.rs`, `src/db.rs`
- [X] T021 Implement `ClientKey`, `TrustedProxies`, `resolve` (research R5; data-model §3.1) in `src/security/client_ip.rs`
- [X] T022 Implement hash, blocking-pool verify, dummy hash, password policy (research R8, R9) in `src/security/password.rs`
- [X] T023 Implement `Claims`, `issue`, `verify` → `Expired | Invalid` (research R6; data-model §2) in `src/security/token.rs`
- [X] T024 Implement `EventSink`, `Event`, stdout and capture sinks (research R17; contracts/security-events.md) in `src/security/events.rs`
- [X] T025 [P] Implement `Clock`, `SystemClock`, `ManualClock` (data-model §3.5) in `src/security/clock.rs`
- [X] T026 Implement `Permits` (data-model §3.4) in `src/security/permits.rs`
- [X] T027 Implement the login and write-check lookups (data-model §1.1), run `sqlx migrate run` on the development database, then `cargo sqlx prepare -- --all-targets`, in `src/security/accounts.rs`, `.sqlx/`
- [X] T028 Implement `Security` with injectable settings, clock, and sink (plan §Configuration, §Helpers) in `src/security/mod.rs`
- [X] T029 Write startup tests: valid `JWT_SECRET` in `spawn_api`, AC 9 cases (plan §Existing tests, §Cases `tests/startup.rs`) in `tests/startup.rs`
- [X] T030 Implement the startup order through `Security::new` (plan §Startup contract) in `src/main.rs`

## Phase 3: US1 — Admin login and authenticated writes (P1, MVP)

- [X] T031 [US1] Rework helpers to seeded accounts (with `SecuritySettings::seed`) and real tokens, delete the test-token file, declare `mod security` with every submodule as an empty file, remove the `#[path = "api/tokens.rs"] mod tokens` include and its check (h) request (plan §Helpers, §Existing tests) in `tests/api/support.rs`, `tests/api/tokens.rs`, `tests/api/main.rs`, `tests/api/security/mod.rs`, `tests/startup.rs`
- [X] T032 [P] [US1] Write login tests for AC 1 with the helper-seeded admin, wrong password → `401 INVALID_CREDENTIALS`, AC 7 (`422` without hashing), and roleless login (plan §Cases `security/login.rs`) in `tests/api/security/login.rs`
- [X] T033 [P] [US1] Write token rejection tests (plan §Cases `security/tokens.rs`) in `tests/api/security/tokens.rs`
- [X] T034 [P] [US1] Write write-authorization tests (plan §Cases `security/write_auth.rs`) in `tests/api/security/write_auth.rs`
- [X] T035 [P] [US1] Replace test-token constants and `401` message expectations (plan §Existing tests `global.rs`) in `tests/api/global.rs`
- [X] T036 [P] [US1] Replace check (h) deny with no-token and forged-token writes → `401 UNAUTHORIZED` (plan §Existing tests `tests/startup.rs`) in `tests/startup.rs`
- [ ] T037 [P] [US1] Remove the retired token and gate checks (plan §Existing tests `tests/repo_hygiene.rs`) in `tests/repo_hygiene.rs`
- [ ] T038 [US1] Add `INVALID_CREDENTIALS`, `SERVICE_UNAVAILABLE` with `Retry-After`, the `401`/`403` messages, and `WWW-Authenticate` on `ApiError` (spec §Error Codes; research R14, R19; contracts/openapi.yaml) in `src/api/error.rs`
- [ ] T039 [US1] Implement `require_admin` (research R16, R19; plan §Request pipeline 4.2) in `src/security/middleware.rs`
- [ ] T040 [US1] Implement `security_headers` replacing `DefaultHeaders` with the 002 CORS headers and `no-store` on non-safe methods (research R15) in `src/security/middleware.rs`
- [ ] T041 [US1] Remove `AdminGate`, `DenyAll`, and their unit tests; add `admin(&req)` and the login handler (plan §Login steps 2, 3, 6–8) in `src/api/auth.rs`
- [ ] T042 [US1] Drop the `gate` parameters from write handlers in `src/api/geologic_time.rs`, `src/api/taxonomy.rs`, `src/api/geography.rs`, `src/api/species/mod.rs`
- [ ] T043 [US1] Change to `app(pool, Arc<Security>)`, wire `security_headers` and `require_admin`, drop the gate from route closures and write helpers, register `/auth/login` in routes and `ROUTES` in `src/api/mod.rs`
- [ ] T044 [US1] Pass `Security` to `api::app` and remove `DenyAll` in `src/main.rs`
- [ ] T045 [US1] Add `/auth/login`, `adminBearer`, `Unauthorized`, `Forbidden`, `LoginOk`, `InvalidCredentials` (contracts/openapi.yaml) in `docs/openapi.yaml`

## Phase 4: US3 — Students fetch within generous limits (P1)

- [ ] T046 [P] [US3] Write GCRA unit tests (data-model §3.2) and the poisoned shard → `500` test (AC 20) in `src/security/rate_limit.rs`
- [ ] T047 [P] [US3] Write general-limit tests for AC 11, 12 (including the ignored `sustained_default_rate`, quickstart §7.1), 15, 26, 27, 28 (plan §Cases `security/rate_limit.rs`) in `tests/api/security/rate_limit.rs`
- [ ] T048 [P] [US3] Update `assert_cors` exposed headers (research R4) in `tests/api/global.rs`
- [ ] T049 [US3] Implement the sharded GCRA table (data-model §3.2) in `src/security/rate_limit.rs`
- [ ] T050 [US3] Add `RATE_LIMITED` with `Retry-After` (spec §Error Codes) in `src/api/error.rs`
- [ ] T051 [US3] Implement the `rate_limit` layer with `RateLimit` / `RateLimit-Policy`, poisoned shard → `500` (research R14), and add the headers to the exposed headers in `security_headers` (research R2, R4, R5) in `src/security/middleware.rs`
- [ ] T052 [US3] Wire `rate_limit` (plan §Request pipeline 3) in `src/api/mod.rs`
- [ ] T053 [US3] Run the latency test with the limiter enabled (plan §Existing tests) in `tests/api/performance.rs`

## Phase 5: US4 — One client cannot exhaust the API (P1)

- [ ] T054 [P] [US4] Write proxy and /64 tests (plan §Cases `security/client_ip.rs`) in `tests/api/security/client_ip.rs`
- [ ] T055 [P] [US4] Write TCP limit tests for AC 16, 17, 30 and the FR-029 idle keep-alive close (plan §Cases `security/limits.rs`) in `tests/api/security/limits.rs`
- [ ] T056 [P] [US4] Write fail-closed tests (plan §Cases `security/fail_closed.rs`) in `tests/api/security/fail_closed.rs`
- [ ] T057 [US4] Add `REQUEST_TIMEOUT`, `URI_TOO_LONG`, `REQUEST_HEADERS_TOO_LARGE` (spec §Error Codes) in `src/api/error.rs`
- [ ] T058 [P] [US4] Add `statement_timeout` of `DB_TIMEOUT` to `api_connect_options` (research R13) in `src/db.rs`
- [ ] T059 [P] [US4] Map transient database errors to `503` (research R14) in `src/api/db_error.rs`
- [ ] T060 [P] [US4] Add the 30 s body deadline → `408` (research R11; plan §Deviations D2) in `src/api/http.rs`
- [ ] T061 [US4] Implement `request_limits` and `capacity` (research R11, R12) in `src/security/middleware.rs`
- [ ] T062 [US4] Run the `require_admin` account lookup under `DB_TIMEOUT` and apply the fail-closed statuses (research R13, R14) in `src/security/middleware.rs`
- [ ] T063 [US4] Wire `request_limits` and `capacity` (plan §Request pipeline 1–2) in `src/api/mod.rs`
- [ ] T064 [US4] Set server timeouts including `keep_alive(15 s)` and pool `acquire_timeout` (plan §Startup contract; research R11, R13) in `src/main.rs`

## Phase 6: US2 — Operator account procedure (P1)

- [ ] T065 [P] [US2] Write operator tool tests, including tool-created admin login for AC 1 (plan §Cases `security/accounts_cli.rs`) in `tests/api/security/accounts_cli.rs`
- [ ] T066 [P] [US2] Allow `tests/api/security/accounts_cli.rs` in `database_url_only_in_allowed_test_files` (plan §Existing tests) in `tests/repo_hygiene.rs`
- [ ] T067 [US2] Implement the operator tool statements (data-model §1.1), then run `cargo sqlx prepare -- --all-targets`, in `src/security/accounts.rs`, `.sqlx/`
- [ ] T068 [US2] Implement `paleo-accounts` (contracts/accounts-cli.md; research R9, R10) in `src/bin/paleo-accounts.rs`

## Phase 7: US5 — Login throttling without user enumeration (P2)

- [ ] T069 [P] [US5] Write sliding-log, reservation, and per-client `rate_limited` dedupe unit tests (data-model §3.3) in `src/security/login_limit.rs`
- [ ] T070 [P] [US5] Write AC 6 identical-response tests with a fresh peer per case and timing tests (plan §Cases `security/login.rs`) in `tests/api/security/login.rs`
- [ ] T071 [P] [US5] Write login-limit tests for AC 14 (plan §Cases `security/rate_limit.rs`) in `tests/api/security/rate_limit.rs`
- [ ] T072 [P] [US5] Write the hashing-bound test for AC 29 (plan §Cases `security/limits.rs`) in `tests/api/security/limits.rs`
- [ ] T073 [US5] Implement per-client and per-username logs with reservations and the per-client `logged` slots (data-model §3.3; research R3) in `src/security/login_limit.rs`
- [ ] T074 [US5] Add login steps 1, 4, 5 (plan §Login; research R3, R8) in `src/api/auth.rs`

## Phase 8: US6 — Security event log (P2)

- [ ] T075 [P] [US6] Write event tests, including `overloaded` and per-reason `rate_limited` dedupe (plan §Cases `security/logging.rs`) in `tests/api/security/logging.rs`
- [ ] T076 [P] [US6] Write `startup_refused` tests (plan §Cases `tests/startup.rs`) in `tests/startup.rs`
- [ ] T077 [US6] Emit `auth_rejected`, `forbidden`, `admin_write`, `rate_limited` (`general`), `overloaded` (contracts/security-events.md) in `src/security/middleware.rs`
- [ ] T078 [US6] Emit `login_succeeded`, `login_failed`, `login_locked`, `rate_limited` (`login_client`, `login_username`, deduped per data-model §3.3) (contracts/security-events.md) in `src/api/auth.rs`
- [ ] T079 [US6] Emit `startup_refused` for the FR-014 refusals (plan §Startup contract) in `src/main.rs`

## Phase 9: US7 — Safe defaults and documented routes (P2)

- [ ] T080 [P] [US7] Write header and preflight tests (plan §Cases `security/headers.rs`) in `tests/api/security/headers.rs`
- [ ] T081 [P] [US7] Write route-inventory tests (plan §Cases `security/inventory.rs`) in `tests/api/security/inventory.rs`
- [ ] T082 [P] [US7] Write the `image_url` no-fetch test (plan §Cases `security/ssrf.rs`) in `tests/api/security/ssrf.rs`
- [ ] T083 [P] [US7] Write per-operation OpenAPI rules (plan §Existing tests `openapi.rs`) in `tests/api/openapi.rs`
- [ ] T084 [P] [US7] Update preflight expectations (research R15) in `tests/api/global.rs`
- [ ] T085 [US7] Add the FR-033 headers and `Server` removal to `security_headers` (research R15) in `src/security/middleware.rs`
- [ ] T086 [US7] Answer preflight with the research R15 methods, headers, and max-age in `src/api/mod.rs`
- [ ] T087 [US7] Merge the remaining responses, headers, and `info.description` changes of contracts/openapi.yaml in `docs/openapi.yaml`

## Phase 10: Polish

- [ ] T088 Check query metadata with `cargo sqlx prepare --check -- --all-targets` and commit `.sqlx/`
- [ ] T089 Run `DATABASE_URL="$TEST_DATABASE_URL" cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` over `src/`, `tests/`
- [ ] T090 Run `cargo audit` and the AC 23 fixture check (quickstart §7.2) against `Cargo.lock`, `tests/fixtures/audit/Cargo.lock`
- [ ] T091 Run the release latency gate and the AC 12 sustained run (quickstart §7.1) in `tests/api/performance.rs`, `tests/api/security/rate_limit.rs`
- [ ] T092 Validate quickstart §2–§8 end to end per `docs/specs/003-security/quickstart.md`
