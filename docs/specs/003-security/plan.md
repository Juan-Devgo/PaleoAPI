# Plan: Spec 003 — Security

**Date:** 2026-10-07, updated 2026-10-08 · **Spec:** [spec.md](./spec.md) · **Design:** [research.md](./research.md), [data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

## Technical Context

- **Base:** the working tree with Spec 002 merged (1c1d5c2). Paths below are verified against it.
- **Dependencies:** `jsonwebtoken` 11.1 (`default-features = false, features = ["rust_crypto"]`, R6); `argon2` 0.6 (default features, R8). Both are the AGENTS §2 stack items "JWT" and "Argon2". `actix-web` keeps its default features. Nothing else: rate limiting, CIDR parsing, CORS, permits, event log, and time formatting use `std` + `serde_json` (R1, R5, R12, R15, R17). Tool: `cargo-audit` 0.22, `cargo install cargo-audit --locked` (R18).
- **No new C dependency:** every crate this feature adds is pure Rust. A C compiler is a build prerequisite: the existing `zstd-sys` (`actix-web` default `compress-zstd`) and `cargo install cargo-audit` compile C (R18, R20).
- **Storage:** migration `accounts` (data-model §1); rate-limit state in process memory (R1).
- **Binaries:** `paleo_api` (server, `default-run`) and `paleo-accounts` (operator tool, contracts/accounts-cli.md).
- **Queries:** the account statements (data-model §1.1) live in `src/security/accounts.rs`, are shared by the API and the tool, and use `query!`. Regenerate `.sqlx/` (002-R3) in this order: run `sqlx migrate run` on the development database so `accounts` exists, run `cargo sqlx prepare -- --all-targets` with the development `DATABASE_URL`, then commit `.sqlx/` together with the queries. Gate: `cargo sqlx prepare --check -- --all-targets` (AGENTS §5 gate 3). Test code uses runtime queries (001 harness), so `.sqlx/` gains entries only for `src/`.
- **Testing:** new cases join the `tests/api` binary (002 harness); TCP-level cases start a real server on `127.0.0.1:0` in a thread with its own `actix_web::rt::System`.

## Constitution Check

**PASS** (pre-design and post-design) against constitution 1.2.0, with one justified deviation (D1, I).

| Principle | How this plan complies |
|---|---|
| I. Easy to Fetch | Reads never touch authorization (§Write authorization); burst 120 / 600 per minute (R2); limits readable from scripts (R4). **Deviation (§Deviations D1):** codec-level `400` / `408` / `431` carry no JSON body, no readable message, and no CORS headers ("plain JSON with predictable shapes", "error messages … explain how to fix it"). Justification: they are produced by the HTTP server before the application sees the request, only for unparsable requests, heads over 128 KiB, or heads not sent within 10 s, none of which a `fetch()` call produces; every response the application produces keeps the envelope (FR-036). Removing it would mean replacing the HTTP server (VI). |
| II. Restricted Writes | One app-level deny-by-default check derived from `ROUTES` (R16); `TestGate`, `DenyAll`, and test tokens removed (FR-016); live role, status, and credential version per write (R7). |
| III. Performance | Rate limiting is one in-memory shard lock per request and never queries the database (R1); the admin check adds one PK lookup to writes only; `performance.rs` runs with the limiter enabled. |
| IV. Test-First | §Test strategy maps every AC to a test file; only AC 12 (10-minute run) and AC 23 (vulnerable fixture) have manual steps (quickstart §7). |
| V. Integrity | Account rules are constraints and a trigger (data-model §1). |
| VI. Simplicity | Two crates, both named by AGENTS §2 (R6). Rate-limit counters, login-attempt counters, and capacity permits live in process memory under the 1.2.0 abuse-control exception (R1, FR-025); losing them only resets the limits. Scaling consequence: limits and lockouts are per process, so one API process per deployment; scaling out requires revisiting R1 (§Risks). |

AGENTS §3/§5 entries for `cargo audit` and `paleo-accounts` are maintained outside this plan.

## Project Structure

```text
Cargo.toml                  # CHANGE: jsonwebtoken (rust_crypto, R6), argon2; default-run = "paleo_api";
                            #   [profile.dev.package.argon2] / [profile.dev.package.blake2] opt-level = 3 (R8)
Cargo.lock                  # CHANGE: new crates, none compiling C (R20)
.cargo/audit.toml           # NEW: [advisories] ignore = ["RUSTSEC-2023-0071"] (§Supply chain)
.env.example                # CHANGE: JWT_SECRET=change-me-…; limit variables commented with defaults
.sqlx/                      # CHANGE: regenerated for src/security/accounts.rs (§Technical Context Queries)
docs/openapi.yaml           # CHANGE: merge contracts/openapi.yaml
docs/specs/002-crud/spec.md # CHANGE: pointers only (§Spec 002 overrides)
migrations/<ts>_accounts.{up,down}.sql   # NEW: data-model §1
src/
├── lib.rs                  # CHANGE: pub mod security
├── config.rs               # CHANGE: security_config_from_env(get) (§Configuration)
├── db.rs                   # CHANGE: StartupError variants; statement_timeout in the existing api_connect_options (R13); DB_TIMEOUT
├── main.rs                 # CHANGE: §Startup contract; DenyAll removed; HttpServer timeouts (R11); pool acquire_timeout
├── bin/paleo-accounts.rs   # NEW: contracts/accounts-cli.md
├── security/
│   ├── mod.rs              # NEW: Security (config, keys, limiters, permits, sink, clock) shared as Arc
│   ├── client_ip.rs        # NEW: ClientKey, TrustedProxies, resolve (R5)
│   ├── rate_limit.rs       # NEW: sharded GCRA table (data-model §3.2)
│   ├── login_limit.rs      # NEW: sliding logs + reservations (data-model §3.3)
│   ├── permits.rs          # NEW: Permits (data-model §3.4)
│   ├── clock.rs            # NEW: Clock, SystemClock, ManualClock (data-model §3.5)
│   ├── token.rs            # NEW: Claims, issue, verify → Expired | Invalid (R6)
│   ├── password.rs         # NEW: hash, verify on blocking pool, dummy hash, policy (R8, R9)
│   ├── accounts.rs         # NEW: account queries (data-model §1.1), shared by API and tool
│   ├── events.rs           # NEW: EventSink, Event, RFC 3339 formatter (R17)
│   └── middleware.rs       # NEW: security_headers, request_limits, capacity, rate_limit, require_admin
└── api/
    ├── mod.rs              # CHANGE: app(pool, Arc<Security>); DefaultHeaders → security middleware (R15);
    │                       #   ROUTES += /auth/login; route closures and write helpers drop the gate; preflight values
    ├── auth.rs             # CHANGE: AdminGate/DenyAll and their unit tests removed; admin(&req) → AdminIdentity | 401;
    │                       #   login handler
    ├── error.rs            # CHANGE: new codes (spec §Error Codes); 401 messages (R19, contracts/openapi.yaml);
    │                       #   WWW-Authenticate / Retry-After on ApiError
    ├── db_error.rs         # CHANGE: transient errors → 503 (R14)
    ├── http.rs             # CHANGE: read_json body deadline 30 s → 408 (R11)
    └── geologic_time.rs, taxonomy.rs, geography.rs, species/mod.rs   # CHANGE: drop the `gate` parameters only
tests/
├── fixtures/audit/Cargo.lock   # NEW: lockfile with smallvec 1.6.0 (RUSTSEC-2021-0003) for AC 23
├── startup.rs              # CHANGE: §Existing tests
├── repo_hygiene.rs         # CHANGE: §Existing tests, §Cases by file
├── db/
│   ├── main.rs             # CHANGE: mod accounts
│   ├── accounts.rs         # NEW: §Cases by file
│   └── support.rs, schema_catalog.rs, migrations.rs   # CHANGE: §Existing tests
└── api/
    ├── main.rs             # CHANGE: `mod tokens;` → `mod security;`
    ├── tokens.rs           # DELETE (FR-016)
    ├── support.rs          # CHANGE: §Helpers
    ├── global.rs, error_map.rs, openapi.rs, performance.rs   # CHANGE: §Existing tests
    └── security/{mod,login,tokens,write_auth,inventory,rate_limit,client_ip,limits,
                  fail_closed,headers,logging,ssrf,accounts_cli}.rs   # NEW: §Cases by file
```

## Configuration (FR-014, FR-020)

Read by `security_config_from_env(get)` after `DATABASE_URL` and before any connection. Unset or empty = default.

| Variable | Default | Valid | Used by |
|---|---|---|---|
| `JWT_SECRET` | required | ≥ 32 bytes; must not contain `change-me` (any case) | R6 |
| `RATE_LIMIT_PER_MINUTE` | 600 | 1–1,000,000 | R2 |
| `RATE_LIMIT_BURST` | 120 | 1–1,000,000 | R2 |
| `LOGIN_LIMIT_PER_CLIENT` | 10 | 1–10,000 | R3 |
| `LOGIN_LIMIT_PER_USERNAME` | 20 | 1–10,000 | R3 |
| `LOGIN_LIMIT_WINDOW_SECONDS` | 900 | 1–86,400 | R3 |
| `TRUSTED_PROXIES` | empty | comma-separated IPv4/IPv6 addresses or CIDRs | R5 |
| `MAX_CONCURRENT_REQUESTS` | 256 | 1–65,535 | R12 |
| `MAX_CONCURRENT_PASSWORD_HASHES` | 2 | 1–64 | R8 |

Code constants: token lifetime 3600 s, leeway 60 s, `DB_TIMEOUT` 2 s, head 10 s, body 30 s, keep-alive 15 s, URI 2,048 bytes, headers 16,384 bytes, limiter capacities (data-model §3). Tests construct `Security` with overridable capacities and clock.

## Startup contract (extends 001-plan §Startup contract)

Order: **`DATABASE_URL` (001 steps 1–2) → security config → connect → migrations → collation → `Security::new` (dummy hash) → bind**. Failures print `paleo_api: <message>`, exit 1, and never print the secret. The three `JWT_SECRET` refusals (FR-014) also emit `startup_refused` (contracts/security-events.md, FR-039); other refusals only print.

| Check | `StartupError` → message |
|---|---|
| `JWT_SECRET` unset/empty | `MissingSecret` → `JWT_SECRET is not set. Set it to a random value of at least 32 bytes (openssl rand -base64 48).` |
| < 32 bytes | `ShortSecret` → `JWT_SECRET is shorter than 32 bytes. Use a random value (openssl rand -base64 48).` |
| contains `change-me` | `PlaceholderSecret` → `JWT_SECRET is still the placeholder from .env.example. Replace it with a random value (openssl rand -base64 48).` |
| limit variable invalid | `InvalidSetting { var }` → `<VAR> must be an integer from <min> to <max>.` |
| `TRUSTED_PROXIES` entry invalid | `InvalidSetting { var }` → `TRUSTED_PROXIES entry '<entry>' is not an IP address or CIDR range.` |

Server: `client_request_timeout(10 s)`, `keep_alive(15 s)`, `client_disconnect_timeout(1 s)`; pool `max_connections(10)`, `acquire_timeout(DB_TIMEOUT)`; bind unchanged.

## Request pipeline (spec §Check Order)

App middleware, outermost first; replaces 002-plan §Request pipeline's `DefaultHeaders` and keeps its `OPTIONS` short-circuit and `ErrorHandlers`.

| # | Layer | Outcome |
|---|---|---|
| — | actix codec | bare `400` / `431` / `408` (§Deviations D1) |
| 0 | `security_headers` | FR-033 headers, CORS, `no-store` on non-safe methods on every response below (R15) |
| 1 | `request_limits` | `414 URI_TOO_LONG`, `431 REQUEST_HEADERS_TOO_LARGE` (R11) |
| 2 | `capacity` | permit held until the response is produced; none → `503`, `Retry-After: 1` (R12) |
| 3 | `rate_limit` | resolves `ClientKey` into extensions (R5); GCRA → `429` or pass; `RateLimit`/`RateLimit-Policy` on every response from here inward (R2, R4) |
| — | 002 `OPTIONS` short-circuit | `204` preflight |
| 4.2 | `require_admin` | R16 decision → token (R6, R19) → account lookup under `DB_TIMEOUT` → `401` / `403` / `503`; stores `AdminIdentity`; logs `auth_rejected`, `forbidden`, `admin_write` |
| 4.1 | `ErrorHandlers` + router | `404` / `405` (decided before 4.2 by R16 so the 002 order holds) |
| 4.3+ | 002 handlers | `read_json` with the 30 s deadline (`408`, §Deviations D2), then 002 steps 3–6 |

- Nothing reads `User-Agent` (FR-027) or, on safe methods, `Authorization` (FR-017).

### Login (`POST /api/v1/auth/login`, spec §Check Order login line)

1. Per-client log (R3) full → `429`; else record.
2. `read_json` → `413` / `415` / `400` / `408`.
3. Object with exactly `username`, `password`, both non-empty strings, password ≤ 128 characters → else `422 VALIDATION_FAILED` (002 details format).
4. Per-username reserve → `429` when full.
5. Hash permit → none: release reservation, `503`, `Retry-After: 1`.
6. `SELECT` (data-model §1.1) under `DB_TIMEOUT` → failure: release, `503` / `500` (R14).
7. Verify on the blocking pool against the stored hash, or the dummy hash for an unknown username (R8).
8. Verified and `active` → `200` (FR-007, token per data-model §2), drop reservation. Otherwise → record failure, `401 INVALID_CREDENTIALS` with `WWW-Authenticate: Bearer realm="paleo-api"` and `no-store`.

A username that is not a slug is treated as unknown (step 7), never `422`, so format checks do not change the timing.

## HTTP surface

Messages and examples: contracts/openapi.yaml. `Retry-After` on every `429` and `503`. `WWW-Authenticate` per R19. Envelope and details: Spec 002 §4.5 unchanged.

## Supply chain (FR-041, R18)

- Gate: `cargo audit` (fetches the RustSec database; `--no-fetch` reuses the local copy).
- Exceptions: `.cargo/audit.toml` `ignore` list. Each entry needs a row here with the advisory, the affected crate, and why it does not apply.

| Advisory | Crate | Why it does not apply |
|---|---|---|
| RUSTSEC-2023-0071 | `rsa` 0.9 (via `jsonwebtoken` `rust_crypto`) | Timing side channel in RSA private-key operations. The API holds no RSA key, accepts only HS256, and never calls RSA code (R6). Revisit if any RSA algorithm is ever accepted. |

## Deviations

- **D1 (FR-033, FR-036, AC 18, AC 30; accepted by the user 2026-10-08, spec wording "the application produces"):** responses produced by the actix-http codec before the application sees the request carry no envelope and no FR-033 headers: `408` when headers do not arrive within 10 s (body empty, then close), `431` for a head over 128 KiB or more than 96 header lines, and `400` for an unparsable request (R11). Every `408`/`414`/`431` the application produces (body deadline; URI > 2,048; headers 16 KiB–128 KiB) has the envelope and headers. Removing D1 would mean replacing the HTTP server.
- **D2 (spec §Check Order step 1, FR-029; accepted by the user 2026-10-08):** the body `408` (30 s deadline) is returned where the body is read: Spec 002 step 3 for writes, login step 2 for login. A request answered at an earlier step (`401`, `403`, `404`, `405`, `429`, `503`) gets that status without its body being read, not `408`. Following the spec order would hold a capacity permit for up to 30 s on every unauthenticated slow-body request before it could be refused (FR-030); no request is served without its body.

## Spec 002 overrides

Spec 002 text gets pointers only, no restated behavior (user decision relayed 2026-10-08; applied by tasks.md T005): §4.4 replaced by FR-015/FR-016; §4.11 rows for `408`/`414`/`429`/`431`/`503` linking to spec §Error Codes; §4.12 pointer to spec §Error Codes; §9 link to this spec.

| Spec 002 | Overridden by |
|---|---|
| §4.4 deny-all gate, test credential | FR-015, FR-016; R16 (`TestGate`, `DenyAll`, `tests/api/tokens.rs` removed) |
| §4.3 CORS preflight methods/headers, exposed headers | FR-034, R15 |
| §4.10 check order | spec §Check Order (prefix steps 1–3) |
| §4.11 `401` / `403` messages; new statuses | FR-018, R19, contracts/openapi.yaml |
| §4.12 error codes | spec §Error Codes |
| §9 deferred security items | this plan |
| 002-R4, 002-R5 (gate inside handlers) | R16 (app-level middleware before routing) |
| 002-R6 `DefaultHeaders` CORS | R15 (`security_headers`) |

## Test strategy

### Rules

- 002-plan §Test strategy rules apply, except writes authenticate with real tokens (§Helpers).
- In-process requests set the client with `TestRequest::peer_addr`; requests without one share key `0.0.0.0`.
- Time-dependent limiter tests use `ManualClock`; token-expiry tests mint tokens with past `iat` (the library reads the system clock).
- AC 27 and no-database assertions use a lazy pool pointed at a closed port: any database access would turn the expected status into `503`.
- Log assertions read the capture sink, never stdout; only `tests/startup.rs` reads the child process's stdout (`startup_refused` is emitted before `Security` exists).

### Existing tests (Spec 001, Spec 002)

| File | Change |
|---|---|
| `tests/db/support.rs` | `ACCOUNT_TABLES = ["accounts"]`; `all_tables()` includes it (seq-scan checks, snapshots, catalog) |
| `tests/db/schema_catalog.rs` | `exactly_the_sixteen_tables_exist` → `exactly_the_seventeen_tables_exist`; `expected_constraints()` gains the six `accounts_*` constraints; triggers gain `accounts_credentials_changed_trg`, functions `accounts_credentials_changed_fn` (data-model §1); `every_table_has_a_truncate_guard` expects `all_tables()` |
| `tests/db/migrations.rs` | `up.len()` 6 → 7 |
| `tests/api/error_map.rs` | `schema_names` excludes `accounts`; `trigger_names` uses `RESOURCE_TABLES` + `LINK_TABLES`, not `all_tables()` (the API never writes `accounts`; the tool reports its errors, contracts/accounts-cli.md) |
| `tests/api/global.rs` | `TEST_ADMIN_TOKEN` / `TEST_USER_TOKEN` → `admin_token` / `user_token` in `writes_need_admin_credentials`, `body_problems_are_400_413_415`, `check_order`, `error_bodies_leak_nothing`; new `401` message; `assert_cors` exposed headers (R4) and preflight values (R15) |
| `tests/api/openapi.rs` | per-operation rules of contracts/openapi.yaml; `/auth/login` in both sets |
| `tests/api/performance.rs` | runs with the limiter enabled (test settings) |
| `tests/startup.rs` | `#[path = "api/tokens.rs"] mod tokens` removed; `spawn_api` sets a valid `JWT_SECRET`; check (h) deny: a write without a token and with a forged token → `401 UNAUTHORIZED` |
| `tests/repo_hygiene.rs` | `test_token_literals`, `test_tokens_are_absent_from_src`, `deny_all_is_the_only_admin_gate_in_src` removed; `database_url_only_in_allowed_test_files` also allows `tests/api/security/accounts_cli.rs` |

### Helpers (`tests/api/support.rs`)

- `app(pool_opts, opts)` keeps its signature: builds `Security` with test settings (limits 1,000,000, capture sink, `SystemClock`) and seeds `admin` (role admin) and `user` (no role) accounts with one precomputed PHC hash literal of a test-only password.
- `send`/`send_as` keep their signatures; tokens come from `admin_token(&security)` / `user_token(&security)` (library `issue`).
- New: `app_with(pool_opts, opts, SecuritySettings)` (`SecuritySettings::seed = false` skips the seeded accounts), `security_env(&[(var, value)])` (goes through `security_config_from_env`), `ManualClock` access, `events(&app)`, `login(app, user, pass, peer)`, `peer(n)`, `b64url(bytes)` (hand-built forged tokens, so no JWT literal is committed).

### Cases by file

| File | Cases (AC) |
|---|---|
| `src/config.rs` unit | defaults; each range edge; empty = default; `TRUSTED_PROXIES` parse errors; secret rules (9) |
| `src/security/*` unit | GCRA formulas and burst (data-model §3.2); sliding log and reservations; `login_client` / `login_username` log dedupe per client within the window (data-model §3.3); client resolution incl. spoofed and malformed XFF, mapped IPv4, /64 (13); password policy (7); claims round trip; RFC 3339 formatter; event escaping: `path` and `client` values containing `\n`, `\r`, `"`, U+001B serialize to one line that parses back to the same values (21); poisoned shard → `500` (20); `Security` construction: settings and capacities carried, injected clock and sink used, dummy hash never verifies |
| `security/login.rs` | unseeded app: no login succeeds; helper-seeded admin → `200` with token shape and `no-store` (1); wrong password → `401 INVALID_CREDENTIALS` with challenge and `no-store`; `422` cases incl. 129 characters without hashing (7); roleless account logs in; unknown/wrong/disabled byte-identical except `Date`, each case from a fresh `peer(n)` under a `ManualClock` so `RateLimit` values match, and median timing within 25% over 50 each with per-username limit raised (6) |
| `security/tokens.rs` | altered payload, removed signature, `alg` `none`/`NONE`, `HS384`, `RS256`, other secret, wrong `iss`/`aud`, missing `exp`/`nbf`/`sub`/`ver`, `exp` 61 s past, `nbf` 61 s ahead → `401` (3); token in query, cookie, body → `401` (4); three distinct `401` messages and challenges (25) |
| `security/write_auth.rs` | write with token → 002 success + `no-store`; no header → `401` + challenge; roleless → `403` (2); disable / password change → `401`, revoke → `403`, re-grant → allowed, enable after disable → still `401` (5); `GET` with garbage/expired header identical to none (8); `404`/`405` before `401` |
| `security/inventory.rs` | every non-safe `(pattern, method)` in `ROUTES` except login → `401` without token; the R16 decision function denies a matched pattern absent from `ROUTES` (10) |
| `security/rate_limit.rs` | burst then `429` + `Retry-After`; after advancing the clock that long → `200`; other peer unaffected (11); 6,000 requests at 100 ms intervals → no `429` (12); configured values enforced (26); `429` with closed-port pool (27); differing `User-Agent` share one limit and identical responses (28); headers on `200`/`304`/`404`/`429` + exposed (15); 11th login per client → `429` even with correct credentials; 20 failures for a username across peers → `429`, unknown username identical (14) |
| `security/client_ip.rs` | XFF from untrusted peer ignored, from trusted proxy used; two addresses in one /64 share a limit (13) |
| `security/limits.rs` (TCP) | 2,049-character URL → `414`; 17 KB headers → `431` (16, 30); one byte/s headers cut within 10 s; complete headers, partial body → `408` with envelope within 30 s (16, 30); after a complete response, an idle kept-alive connection is closed by the server within 15 s plus margin (FR-029); `MAX_CONCURRENT_REQUESTS=1` with a request blocked on a row lock → `503` + `Retry-After: 1`, then normal (17); write blocked on a lock > 2 s → `503`, no SQL in body (17); `MAX_CONCURRENT_PASSWORD_HASHES=1` with the permit held → login `503` (29) |
| `security/fail_closed.rs` | account lookup on an unreachable pool → `503`; limiter at capacity → `503`; neither runs the handler (20, 30) |
| `security/headers.rs` | FR-033 set and no `Server` on `200`, `304`, `4xx`, `429`, `5xx` (18); preflight for `POST` + `Authorization` → `204` with R15 values; no `Allow-Credentials` anywhere (19) |
| `security/logging.rs` | scenario run → one event of each kind; password, token, and secret absent from all captured lines; login with a username containing `\n` → `401` and exactly one new line, without `account` (21; the escaping itself is the `events.rs` unit case, as no request-derived field can carry a raw control character); two `503`s within one second with the capacity permit held → one `overloaded` line, `reason` `capacity`; after advancing the clock 1 s, the next `503` → a second line (FR-039); from one peer under a `ManualClock`, repeated `429`s for each `reason` (`general`, `login_client`, `login_username`) → one `rate_limited` line per reason; the next `429` after advancing the clock past that reason's window (`w`, `LOGIN_LIMIT_WINDOW_SECONDS`) → a second line (FR-039) |
| `security/ssrf.rs` | species with `image_url` at a local listener → no connection accepted (22) |
| `security/accounts_cli.rs` | binary via `CARGO_BIN_EXE_paleo-accounts`: create, duplicate, invalid slug (24); 11/129 characters, equal to username (7); TTY refusal; every command's outcome row; tool-created admin logs in through the API → `200` (1) |
| `tests/db/accounts.rs` | duplicate PK, invalid slug, role/status outside values, malformed hash → named constraints (24); trigger bumps version on password and status, not on role; login and write-check lookups (data-model §1.1): unknown username → none, existing row → its fields |
| `tests/startup.rs` | missing, 31-byte, placeholder secret → exit 1, secret absent from output (9), stdout has one `startup_refused` line with the matching reason (FR-039); invalid limit variable names the variable, no `startup_refused` |
| `tests/repo_hygiene.rs` | `.env.example` `JWT_SECRET` contains `change-me`; no `TestGate`, `DenyAll`, `TEST_ADMIN_TOKEN`, or JWT-shaped literal (`eyJ…\.eyJ`) in tracked files outside `docs/` and `tests/repo_hygiene.rs` itself (pathspecs `:!docs`, `:!tests/repo_hygiene.rs`, as in `no_credentialed_urls_except_placeholders`) (FR-016); each `.cargo/audit.toml` ignore has a §Supply chain row |
| `tests/api/openapi.rs` | `/auth/login` POST with `security: []`; per-operation rules of contracts/openapi.yaml (38) |
| manual | AC 12 sustained 10-minute run; AC 23 fixture (quickstart §7) |

## Risks

| Risk | Mitigation |
|---|---|
| More than one API process: limits multiply, lockouts are per process | single process documented (R1); revisit before scaling out |
| Many IPv6 /64s fill the limiter table | fail closed `503` for new clients only; entries expire within `burst / rate` (12 s by default) |
| `TRUSTED_PROXIES` unset behind a proxy: every student shares the proxy's address | quickstart §6; burst sized for a shared address |
| Kept-alive connections: slow later heads are bounded by keep-alive (15 s), not 10 s | accepted; first request is bounded by 10 s (R11) |
| Network hang inside a 002 handler not covered by a client-side timer | server-side `statement_timeout` + `acquire_timeout` (R13); OS TCP timeouts |
| Slow-body clients holding capacity on login | per-client login limit bounds it to 10 per window per client |
| Timing test (AC 6) flaky | `opt-level = 3` for argon2/blake2 in dev; medians; interleaved order |
| The RUSTSEC-2023-0071 exception hides a future RSA use | HS256-only `Validation`; `RS256` token test → `401`; §Supply chain row names the revisit condition |
| Admin delayed up to 15 min by a username lockout | accepted in spec §Assumptions |

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| Hand-written GCRA and sliding logs | burst semantics + exact login windows + spoof-proof keys | `governor` covers only GCRA, emits legacy headers (R1) |
| `jsonwebtoken` with `rust_crypto` | vetted JWT validation in pure Rust; one recorded audit exception | `aws_lc_rs` adds a new C dependency (R20); an own `CryptoProvider` adds two direct dependencies and hand-written crypto glue (R6) |
| `argon2` | AGENTS §2 hashing | — |
| `paleo-accounts` binary | hashing must run in Rust; no HTTP account surface (FR-002) | raw SQL needs external hashing (R10) |
| App-level `require_admin` from `ROUTES` | deny by default without per-handler opt-in, 002 order kept | per-handler gate violates FR-015; resource middleware breaks `405` (R16) |
| Application `414`/`431` below codec limits | envelope + headers (FR-036) | codec limits are fixed and bare (R11) |
| `credentials_version` + trigger | exact invalidation (FR-005) | timestamp comparison races (R7) |
| `EventSink` trait object | capture events in tests (AC 21) | stdout capture is racy across parallel tests |
