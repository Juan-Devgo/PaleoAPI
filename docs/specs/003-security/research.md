# Research — Spec 003

**Date:** 2026-10-07, updated 2026-10-08 (R6, R18, R20). "Checked" = read in the locked crate sources (`actix-http` 3.13.3, `actix-web` 4.15.0) or in the working tree with Spec 002 merged (1c1d5c2; its docs cited as `002-plan`, `002-R<n>`).

## R1. Where rate-limit counters live (Constitution Check VI)

- **Decision:** in process memory, in sharded `std::sync::Mutex<HashMap>` tables owned by the API process (data-model §3). Nothing about limits is written to PostgreSQL.
- **Why:** FR-025 / AC 27 require that a rejected request does no database work, so the counter cannot be in the database: reading it is database work. A counter write per request would also put one pool connection and one round trip on every read (Constitution III) and would let a flood exhaust the 10-connection pool, the opposite of US4. The spec classes the counters as disposable abuse-control state (Key Entities): losing them only resets windows.
- **Accepted consequences:** a restart resets every window; with N API replicas each enforces its own limits (effective limit ×N). Hosting is out of scope and the deployment is one process (001-plan §Local database service); scaling out requires revisiting this (move limits to the edge or to a shared store).
- **Rejected:** PostgreSQL `UNLOGGED` table (violates FR-025, adds a write per read); Redis or another store (new service, Constitution VI); `governor`/`actix-governor` crates (GCRA only, so the login sliding windows still need custom code; legacy `X-RateLimit-*` headers; trusts no proxy model of FR-019; three extra crates for ~150 lines).

## R2. General limit: algorithm and burst (FR-021, FR-023, FR-024)

- **Decision:** GCRA (token bucket equivalent) per client: rate `RATE_LIMIT_PER_MINUTE` = 600 (one token every 100 ms), bucket `RATE_LIMIT_BURST` = **120**. Formulas in data-model §3.2.
- **Why 120:** a classroom of 30 students sharing one address that loads a page making ~4 requests at the same moment sends 120 requests at once; sustained use stays under 10 req/s. 120 tokens refill in 12 s, so a flood from one client is capped at 120 + 10/s. A sliding window of 600/min would allow 600 requests in one second.
- **Rejected requests do not extend the wait** (FR-023): GCRA only advances the bucket on accepted requests.
- **Rejected:** fixed window (2× burst at window edges); sliding log for 600 events per client (memory); burst = 600 (one client can send 600 requests in a second).

## R3. Login limits (FR-022)

- **Decision:** exact sliding logs: per client the instants of the last ≤ 10 login requests, per username the instants of the last ≤ 20 failed logins, both within `LOGIN_LIMIT_WINDOW_SECONDS` = 900. A request is rejected when the log is full; `Retry-After` = oldest instant + window − now. Rejected requests are not recorded.
- **Per-username key:** a 64-bit SipHash of the submitted username with a per-process random key (`std::hash::RandomState`), so unknown, invalid, and very long usernames cost the same bounded memory and are counted identically. The username slot is reserved before hashing and released on success or on a `503`, so parallel guesses cannot overshoot the limit.
- **Why exact logs:** AC 14 requires "the 11th attempt within 15 minutes" to fail; a token bucket with the same rate lets 11 through after 90 s.
- **Accepted:** the per-username lockout lets anyone delay an admin's login by up to 15 minutes (spec Assumptions).

## R4. Rate-limit response headers (FR-023, FR-024)

- **Decision:** `draft-ietf-httpapi-ratelimit-headers-11` fields, one policy named `"general"`:
  - `RateLimit-Policy: "general";q=120;w=12` — the bucket: `q` = burst, `w` = ⌈burst × 60 / per-minute⌉ seconds (= 600/min with the defaults).
  - `RateLimit: "general";r=<tokens available now>;t=<seconds until the bucket is full again>`.
  - `Retry-After: <delay-seconds>` (RFC 9110 §10.2.3) on every `429` and `503`, always an integer ≥ 1. The draft gives `Retry-After` precedence over `RateLimit`.
- **Why:** the draft is algorithm-agnostic and its own example names a `"burst"` policy; advertising the bucket states exactly what is enforced. No `pk` parameter (draft privacy note). Login limits are reported only through `Retry-After` and the message; their state is not advertised.
- **Exposed to scripts:** `Access-Control-Expose-Headers: ETag, Retry-After, RateLimit, RateLimit-Policy` (FR-024).
- **Rejected:** legacy `X-RateLimit-*` (non-standard); `q=600;w=60` with `r ≤ 120` (`r` could never reach `q`, which misleads clients).

## R5. Client identification (FR-019)

- **Decision:** client = `req.peer_addr()`. Only when the peer is inside `TRUSTED_PROXIES` (comma-separated IPs/CIDRs, empty by default) is `X-Forwarded-For` read: all field lines in order, entries walked right to left, skipping trusted proxies; the first untrusted entry is the client. An unparsable entry stops the walk and the hop to its right is used; a missing header means the peer. IPv4-mapped IPv6 is folded to IPv4; IPv6 keys are the /64 prefix. No peer (in-process tests) → `0.0.0.0`.
- **Why:** rightmost-untrusted is the only spoof-proof reading; the leftmost entries are client-controlled. actix's `ConnectionInfo::realip_remote_addr()` trusts `Forwarded`/`X-Forwarded-For` from any peer (checked, `info.rs`), so it is never used. `Forwarded` (RFC 7239) is ignored: common proxies (nginx, Caddy, Traefik) set `X-Forwarded-For`.
- **Rejected:** `ipnet` crate (CIDR containment is a few lines over `std::net`).

## R6. JWT library and validation (FR-011–FR-014)

- **Decision:** `jsonwebtoken` 11.1 with `default-features = false, features = ["rust_crypto"]` (pure-Rust RustCrypto backend); HS256 only.
  - Header `{"alg":"HS256","typ":"JWT"}`; claims `sub` (username), `iss` = `aud` = `"paleo-api"`, `iat`, `nbf` = `iat`, `exp` = `iat + 3600`, `ver` (credential version, R7).
  - `Validation`: `algorithms = [HS256]`, `leeway = 60`, `validate_nbf = true`, `set_issuer`, `set_audience`, `required_spec_claims = {exp, nbf, iss, aud, sub}`; `iat` and `ver` are non-optional fields of the claims struct, so a token without them fails to decode. Tokens over 4 KB are rejected before decoding.
  - Error mapping: `ExpiredSignature` (only produced after the signature verified) → expired; every other error → invalid.
- **RFC 8725 / 8725bis coverage:** single allowed algorithm, case-sensitive `alg` (8725bis §2.11, §3.1) — the library compares the parsed enum against the allow-list; `none` is not an `Algorithm` variant, so unsigned tokens fail to parse (§3.2); HMAC key is a ≥ 32-byte random secret, never a password (§3.5, FR-014); `iss`/`aud`/`sub` always validated (§3.8–§3.9). Explicit typing (§3.11) is not needed: one key signs one kind of JWT, and the key is used for nothing else.
- **Backend:** `rust_crypto`. HS256 runs on RustCrypto `hmac` + `sha2`; it adds no C dependency (R20). The feature also compiles `rsa`, `p256`, `p384`, `ed25519-dalek`, and `rand`; they are never called, because the only key is `DecodingKey::from_secret` and `Validation.algorithms = [HS256]`.
- **RUSTSEC-2023-0071 (`rsa`, Marvin timing attack, no patched version as of 2026-09-14):** it affects RSA private-key operations. The API holds no RSA key and never runs RSA code, so the advisory does not apply. It is the one recorded exception of plan §Supply chain (FR-041).
- **AGENTS §2:** the stack names "JWT" and "Argon2". `jsonwebtoken` and `argon2` are the only new direct dependencies. `rust_crypto` is a feature of the JWT crate, not a new dependency.
- **Rejected:**
  - `aws_lc_rs` backend: builds `aws-lc-sys` from C, a new C dependency (R20). The user keeps `rust_crypto`.
  - Own `CryptoProvider` built on `hmac` + `sha2` (the crate supports this when no backend feature is enabled): it avoids `rsa` and the audit exception, but adds two direct dependencies outside AGENTS §2 and hand-written signer/verifier/JWK glue on the verification path.
  - Hand-written HS256: security-critical parsing and validation code with no vetting.

## R7. Invalidating tokens on credential change (FR-005, FR-013)

- **Decision:** `accounts.credentials_version` (integer). A trigger bumps it (and `credentials_changed_at`) when the password hash or the status changes; the token carries it as `ver`; every write compares `ver` with the stored value. Role changes do not bump it, so removing the role gives `403` (AC 5) and re-granting restores access while the token lives.
- **Why:** comparing `iat` (whole seconds) with a microsecond timestamp misclassifies tokens issued in the same second as the change, in both directions. An integer is exact and needs no clock.
- **Disable bumps the version:** re-enabling an account does not revive tokens issued before it was disabled.
- **Rejected:** `iat ≥ credentials_changed_at` (above); revocation list (spec Out of Scope); role in the token (FR-005 needs the live role).

## R8. Password hashing (FR-001, FR-008, FR-032)

- **Decision:** `argon2` 0.6 (default features), Argon2id v19, `m = 19456 KiB, t = 2, p = 1`, 16-byte random salt, PHC string storage. Verification runs on the blocking pool (`actix_web::rt::task::spawn_blocking`, a tokio `spawn_blocking`, so it also runs under the `#[sqlx::test]` runtime of 002-R11).
- **Equal cost for unknown usernames:** at startup the API hashes a random 32-character password with the same parameters; unknown, invalid, and disabled-account logins verify the submitted password against that hash or the real one, so every `401` costs one Argon2 verification.
- **Concurrency bound:** `MAX_CONCURRENT_PASSWORD_HASHES` = **2** permits (atomic counter with an RAII guard, no queue); no permit → `503` + `Retry-After: 1`. 2 × 19 MiB of hashing memory; ≈ 40 logins/s on one core per permit at ~50 ms each, enough for < 10 admins (spec Assumptions).
- **Debug builds:** `[profile.dev.package.argon2]` and `[profile.dev.package.blake2]` get `opt-level = 3`, otherwise one verification takes seconds in `cargo test`.
- **Rejected:** OWASP's 46 MiB/t=1 variant (more memory per concurrent login); `tokio::sync::Semaphore` (needs `tokio` as a normal dependency for a counter); queuing logins (FR-032 requires `503`).

## R9. Password policy (FR-004)

- **Decision:** length in Unicode scalar values (`chars().count()`), 12–128; equality with the username compared case-insensitively (usernames are lowercase slugs); no Unicode normalization (a new crate for a corner case of operator-typed passwords).
- **Where:** in the operator tool (the database never sees the plaintext). The login rejects > 128 before hashing (FR-009).

## R10. Operator procedure (FR-002, FR-003)

- **Decision:** a second binary `paleo-accounts` in the same crate (contracts/accounts-cli.md). Arguments parsed by hand; the password is read from standard input, never from arguments or the environment; an interactive terminal on stdin is refused with the `read -rs` recipe, so the password is never echoed. Uses `DATABASE_URL` and the 001 startup checks (connect, pending migrations).
- **Why:** Argon2 hashing must run in Rust, so plain `psql` cannot create a password; a separate binary keeps account management out of the HTTP surface (FR-002) and out of the server's argument handling.
- **Rejected:** `clap` and `rpassword` crates (seven subcommands; `read -rs` hides input); HTTP admin endpoints (FR-002); SQL + external hashing tool (operator must pick Argon2 parameters by hand).

## R11. Connection and request limits in actix (FR-028, FR-029)

Checked in `actix-http` 3.13.3 `h1/decoder.rs` and `h1/dispatcher.rs`:

- The codec rejects a request head larger than 128 KiB (`MAX_BUFFER_SIZE`) or with more than 96 header lines (`MAX_HEADERS`) with a **bare** `431` (empty body, no application middleware), and an unparsable request with a bare `400`.
- `HttpServer::client_request_timeout` expires as a **bare** `408` followed by a close; it applies to the first request of a connection. Later requests on a kept-alive connection are bounded by the keep-alive timer, which closes the connection.
- **Decision:**
  - `client_request_timeout(10 s)` (FR-029 headers). `keep_alive(15 s)` (FR-029 idle; a proxy's upstream keep-alive must be shorter). `client_disconnect_timeout(1 s)`.
  - An application middleware returns `414 URI_TOO_LONG` when the request target is longer than 2,048 bytes and `431 REQUEST_HEADERS_TOO_LARGE` when Σ(name + value + 4) over all header lines exceeds 16,384 bytes, both with the envelope and the FR-033 headers.
  - The 30-second body deadline lives where bodies are read (`api::http::read_json`, 002-R5): `timeout(30 s, payload.to_bytes_limited(65_536))` → `408 REQUEST_TIMEOUT` with the envelope; actix then closes the connection because the body is unread. A request answered before its body is read (for example `401`) is never held for its body.
- **Deviation (plan §Deviations D1):** the three codec-level responses above cannot carry the envelope or FR-033 headers without replacing the HTTP server.

## R12. Processing bound (FR-030)

- **Decision:** a global in-flight counter shared by all workers; above `MAX_CONCURRENT_REQUESTS` = **256** the request gets `503` + `Retry-After: 1` immediately, never queued.
- **Why 256:** with a 10-connection pool and reads of a few ms, 256 waiting requests drain in well under a second; beyond that, queueing only adds latency for everyone.
- **Rejected:** actix `max_connections` (counts connections, and over-limit connections wait in the accept backlog instead of getting `503`).

## R13. Database time limit (FR-031)

- **Decision:** `DB_TIMEOUT` = **2 s** (code constant): `statement_timeout = 2000` on every API connection (`db::api_connect_options`, server-side, also cancels lock waits and deferred-constraint `COMMIT`s) and `acquire_timeout(2 s)` on the pool. The account lookup of the admin check also runs under `timeout(2 s)` on the client side.
- **Why 2 s:** 40× the read p95 target; a write's statements wait on row locks for at most this long before the client gets `503`.
- **Residual:** a silently dropped TCP connection inside a 002 handler is detected by the OS, not by a client-side timer (accepted; documented in plan §Risks).

## R14. Fail closed: `500` vs `503` (FR-036)

| Failure | Status | Why |
|---|---|---|
| Pool timeout/closed, I/O or TLS error, SQLSTATE `57014`, `57P01`–`57P03`, `53300`, class `08` | `503 SERVICE_UNAVAILABLE`, `Retry-After: 5` | transient: the database is slow, restarting, or full |
| In-flight bound, password-hash bound, limiter table full after cleanup | `503`, `Retry-After: 1` | load |
| Token decoding error of any kind | `401` (invalid) | verification failed = deny |
| Poisoned limiter lock, blocking-task join error, stored hash not parsable, any other database error | `500 INTERNAL_ERROR` | defect; retrying does not help |

Every case returns before the handler runs or before the write, never passes the request through.

## R15. Security headers and CORS (FR-033–FR-035)

- **Decision:** one outermost middleware (replacing 002's `DefaultHeaders`, 002-R6) sets on every application response: `X-Content-Type-Options: nosniff`, `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`, `Referrer-Policy: no-referrer`, **`Strict-Transport-Security: max-age=63072000; includeSubDomains`**, `Access-Control-Allow-Origin: *`, the exposed headers of R4, and `Cache-Control: no-store` on every response to a method other than `GET`, `HEAD`, `OPTIONS` (login and writes, FR-035). It removes any `Server` header (actix sends none; checked).
- **HSTS value:** OWASP's recommended two years with subdomains. `preload` is left out because it is a commitment for the registrable domain, which the API does not own. Browsers ignore HSTS on plain-HTTP responses (RFC 6797 §8.1), so local `http://127.0.0.1:8000` use is unaffected.
- **Preflight:** 002's `OPTIONS` short-circuit stays and now answers `Access-Control-Allow-Methods: GET, POST, PATCH, DELETE`, `Access-Control-Allow-Headers: Authorization, Content-Type, If-None-Match`, `Access-Control-Max-Age: 86400`. Never `Access-Control-Allow-Credentials`.
- **Rejected:** `actix-cors` (002-R6 reasons still hold).

## R16. Write authorization placement (FR-015, FR-017)

- **Decision:** an app-level middleware decides before routing, using `api::ROUTES`: safe methods and `POST /api/v1/auth/login` pass; a path matching a `ROUTES` pattern with an unlisted method passes (router answers `405`); a path matching no pattern passes only if `req.match_pattern()` is `None` (router answers `404`), otherwise it is denied (a registered route missing from `ROUTES` is still protected). Everything else needs a valid token. The middleware stores `AdminIdentity` in the request extensions; 002's write helpers check for it (`api::auth::admin(&req)`) instead of calling the gate, so a handler reached without the middleware still fails closed.
- **Why:** 002-R5 rejected a gate middleware because a resource-level one answers `401` before `405`; deciding at app level against the route table keeps spec 002 §4.10 steps 1→2 and removes the per-handler opt-in that FR-015 forbids. `match_pattern` falls back to the resource map before routing (checked, `request.rs`).
- **Replaces:** 002-R4 `AdminGate`/`DenyAll`/`TestGate` and the test tokens (FR-016).

## R17. Security event log (FR-039, FR-040)

- **Decision:** one JSON object per line on standard output, written with `serde_json` (already a dependency) through an `EventSink` trait object (stdout in production, an in-memory buffer in tests). Schema: contracts/security-events.md. Time is RFC 3339 UTC with milliseconds, formatted from `SystemTime` by a small civil-date function.
- **Escaping:** JSON string encoding escapes every control character, so a client value cannot end the line (FR-040). Usernames are logged only when they name an existing account, so a password typed into the username field never reaches the log.
- **Rejected:** `tracing`/`log` + subscriber crates (three crates for one event stream); plain text (needs custom escaping).

## R18. Supply-chain gate (FR-041)

- **Decision:** `cargo-audit` 0.22 (RustSec advisory database) run as `cargo audit` in the quality gates. Exceptions live in `.cargo/audit.toml` `[advisories] ignore`, each with a comment pointing at plan §Supply chain, where the reason is recorded. One exception at planning time: RUSTSEC-2023-0071 (R6).
- **Installing the tool (user decision 2026-10-08):** `cargo install cargo-audit --locked` (AGENTS §3). It compiles `aws-lc-sys` (default `rustsec` feature `gix-reqwest` → `reqwest` 0.13 `rustls` → `aws-lc-rs`), so it needs a C compiler (R20). The tool is not a dependency of the crate, so this adds no C dependency to the build.
- **Rejected:** `cargo-deny` (also licenses/bans/sources; more configuration than FR-041 needs).

## R19. `401` and `WWW-Authenticate` (FR-015, FR-018)

- **Decision:** no `Authorization` header or a non-`Bearer` scheme → *missing*: `WWW-Authenticate: Bearer realm="paleo-api"` (RFC 6750 §3: no error code when no credentials were sent). A `Bearer` credential that fails → *expired* or *invalid*: `Bearer realm="paleo-api", error="invalid_token"`. Login `401 INVALID_CREDENTIALS` also carries `Bearer realm="paleo-api"` because RFC 9110 §15.5.2 requires a challenge on every `401`. `403` carries no challenge.
- Tokens in the query string, cookies, or the body are never read (RFC 6750 §2.3, §5.3; FR-012).

## R20. C dependencies (user decisions 2026-10-08)

- **Finding (checked, `Cargo.lock` at 1c1d5c2):** the only crate that compiles C is `zstd-sys` (via `cc`). It comes from `actix-web`'s default feature `compress-zstd` → `actix-http/compress-zstd`. Its build script compiles libzstd with `cc::Build` unless pkg-config is opted into. `libsqlite3-sys` is in the lockfile without `cc` (no bundled build), so it compiles no C.
- **Decision:** `actix-web` keeps its default features, including `compress-zstd` (except `http2`, removed by R21). A C compiler is an accepted build prerequisite (for `zstd-sys` and for installing cargo-audit, R18). The rule for this feature: it adds no new C dependency. `jsonwebtoken` uses `rust_crypto` (R6) and `argon2` is pure Rust.
- **Rejected:** dropping `compress-zstd` and asserting "no `cc` package in `Cargo.lock`" in `tests/repo_hygiene.rs` (the user keeps the default features and accepts a C compiler); `ZSTD_SYS_USE_PKG_CONFIG` (needs a system libzstd and still links C).

## R21. RUSTSEC-2026-0258 and `actix-web`'s `http2` feature (user decision 2026-10-08)

- **Finding (checked, `Cargo.lock`, actix-web 4.15.0, actix-http 3.13.3 sources):** `cargo audit` reports RUSTSEC-2026-0258 for `h2` 0.3.27 (server-side DoS via unbounded empty DATA frames; patched ≥ 0.4.16). `actix-http` is the only dependent of `h2`, through `actix-http/http2`, which only `actix-web`'s default `http2` feature enables (`actix-http` defaults are empty). `actix-http` 3 depends on `h2` 0.3, so no patched version can be selected.
- **Reachability:** compiled in, but in the current server reachable only through a listener that selects HTTP/2: `HttpServer::bind` uses `HttpService::tcp()`, which is HTTP/1.x only; HTTP/2 prior knowledge on plain TCP is accepted only by `bind_auto_h2c`/`listen_auto_h2c` (`tcp_auto_h2c` peeks for `PRI * HTTP/2`), and HTTP/2 over TLS only by the `rustls-*`/`openssl` listeners (ALPN `h2`). `src/main.rs` uses `bind`. A `PRI * HTTP/2.0` preface on the HTTP/1 codec fails parsing (bare `400`, R11).
- **Decision:** `actix-web` with `default-features = false` and every other 4.15 default re-enabled (`macros`, `compress-brotli`, `compress-gzip`, `compress-zstd`, `cookies`, `unicode`, `compat`, `ws`). `h2` leaves the build, the FR-041 gate passes without an ignore entry, and an HTTP/2 listener can no longer be enabled by accident (those APIs do not compile without `http2`). Keeping the other defaults leaves all HTTP/1 behavior and R20's `zstd-sys` finding unchanged; pruning defaults the app does not use is out of scope. **Reverses R20's "`actix-web` keeps its default features"** for `http2` only.
- **Rejected:** an `.cargo/audit.toml` ignore (the code would stay in the binary, and one `auto_h2c` or TLS listener would make it reachable); upgrading `h2` (blocked by `actix-http` 3's `h2` 0.3 requirement).

## Sources

- OWASP API Security Top 10 2023: https://api-security.owasp.org/editions/2023/en/0x11-t10/ ; API4:2023 Unrestricted Resource Consumption: https://api-security.owasp.org/editions/2023/en/0xa4-unrestricted-resource-consumption/
- RFC 8725 JWT Best Current Practices: https://www.rfc-editor.org/rfc/rfc8725
- draft-ietf-oauth-rfc8725bis-10 (2026-08-21): https://datatracker.ietf.org/doc/draft-ietf-oauth-rfc8725bis/
- RFC 6750 Bearer Token Usage (§2.3, §3, §3.1, §5.3): https://www.rfc-editor.org/rfc/rfc6750
- draft-ietf-httpapi-ratelimit-headers-11 (2026-05-23): https://datatracker.ietf.org/doc/html/draft-ietf-httpapi-ratelimit-headers-11
- RFC 9110 HTTP Semantics (§10.2.3 Retry-After, §15.5.2 401): https://www.rfc-editor.org/rfc/rfc9110
- RFC 6797 HSTS (§8.1): https://www.rfc-editor.org/rfc/rfc6797
- OWASP Password Storage Cheat Sheet (Argon2id parameters): https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html
- OWASP HTTP Headers Cheat Sheet (HSTS value, Server header): https://cheatsheetseries.owasp.org/cheatsheets/HTTP_Headers_Cheat_Sheet.html
- jsonwebtoken 11.1.0 features, backends, and `Validation`: https://docs.rs/crate/jsonwebtoken/11.1.0/source/Cargo.toml.orig , https://docs.rs/crate/jsonwebtoken/11.1.0 (README: "at most one" backend; none → own `CryptoProvider`), https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/crypto/struct.CryptoProvider.html , https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/struct.Validation.html
- argon2 0.6.0 features: https://docs.rs/crate/argon2/0.6.0/source/Cargo.toml.orig
- RUSTSEC-2023-0071 (`rsa`, affected operations, no patched version, modified 2026-09-14): https://rustsec.org/advisories/RUSTSEC-2023-0071.html
- actix-web 4.15.0 default features: https://docs.rs/crate/actix-web/4.15.0/source/Cargo.toml.orig
- zstd-sys 2.0.16 build script (`cc::Build` by default): https://docs.rs/crate/zstd-sys/2.0.16+zstd.1.5.7/source/build.rs
- cargo-audit 0.22.2 → rustsec 0.33 (`gix-reqwest` default) → gix-transport (`reqwest/rustls`) → reqwest 0.13.4 (`rustls` = `__rustls-aws-lc-rs`): https://docs.rs/crate/cargo-audit/latest/source/Cargo.toml.orig , https://docs.rs/crate/rustsec/latest/source/Cargo.toml.orig , https://docs.rs/crate/gix-transport/latest/source/Cargo.toml.orig , https://docs.rs/crate/reqwest/0.13.4/source/Cargo.toml.orig
- actix-http 3.13.3 `h1/decoder.rs`, `h1/dispatcher.rs`; actix-web 4.15.0 `request.rs`, `types/payload.rs` (local crate sources, checked)
- RUSTSEC-2026-0258 (`h2` < 0.4.16, unbounded empty DATA frames, 2026-08-18): https://rustsec.org/advisories/RUSTSEC-2026-0258.html
- actix-web 4.15.0 `Cargo.toml` `[features]` (`default`, `http2 = ["actix-http/http2"]`), `server.rs` (`listen` → `.tcp()`, `listen_auto_h2c` behind `http2`); actix-http 3.13.3 `Cargo.toml` (`default = []`, `http2 = ["dep:h2"]`), `service.rs` (`tcp()` HTTP/1.x only, `tcp_auto_h2c` preface peek) (local crate sources, checked)
