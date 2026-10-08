# Spec 003 — Security

**Status:** Approved · **Updated:** 2026-10-08 · **Constitution:** 1.2.0 · **Depends on:** [Spec 001](../001-database/spec.md), [Spec 002](../002-crud/spec.md)

This spec delivers what Spec 002 §4.4 and §9 defer to "the security spec": admin authentication, the admin role check, and rate limiting. It replaces the deny-all admin check of Spec 002 §4.4 and adds abuse protection and hardening for the whole API. Resource contracts in Spec 002 are unchanged. Error envelopes, body rules, and status-code conventions are those of Spec 002 §4.5, §4.10–§4.12.

## Clarifications

- 2026-10-07: Accounts are managed only by a documented operator procedure run outside the HTTP API; there are no account endpoints (FR-002).
- 2026-10-07: `admin` is the only role. An account either has it or has no role; a roleless account can log in but its writes return `403` (FR-001, FR-002).
- 2026-10-07: The default general limit is 600 requests per minute per client, sized for a classroom sharing one address (FR-021).
- 2026-10-08: FR-033, FR-036, AC 18, and AC 30 cover every response the application produces. Responses the HTTP server sends before the application receives the request are outside them.

## Motivation

- Constitution II: writes must never be exposed without authentication and role checks. Today every write is rejected (Spec 002 §4.4), so the dataset cannot be curated.
- Constitution I: reads are public and anonymous, so the API is a permanent target for bots, scrapers, and broken client loops. One client must not be able to degrade the service for every student.
- A public API with an admin login is exposed to the known API risk classes (§Threat Model). Each one is either covered by a requirement here or explicitly out of scope.

## Threat Model

Sources: OWASP API Security Top 10 (2023, latest API edition) and OWASP Top 10:2025 for classes the API list does not cover.

| Threat | Applies to PaleoAPI | Covered by |
|---|---|---|
| API1 Broken Object Level Authorization | No: no per-user objects; all data is public and writes are all-or-nothing admin. | — |
| API2 Broken Authentication | Yes: login brute force, credential stuffing, forged or replayed tokens, user enumeration. | FR-007–FR-018, FR-022 |
| API3 Broken Object Property Level Authorization | Partly: mass assignment is already rejected by Spec 002 §4.10 (unknown and response-only fields → `422`). Account data is never exposed over HTTP. | FR-001, FR-002, Spec 002 §4.10 |
| API4 Unrestricted Resource Consumption | Yes: request floods, slow connections, oversized requests, expensive password hashing, long queries. | FR-019–FR-032, Spec 002 §4.6, §4.10 |
| API5 Broken Function Level Authorization | Yes: a write route that forgets the admin check. | FR-015, FR-016 |
| API6 Unrestricted Access to Sensitive Business Flows | Scraping only. The data is public by mission; scraping within limits is allowed. | FR-021, FR-027 |
| API7 Server-Side Request Forgery | Latent: `image_url` is client-supplied. | FR-037 |
| API8 Security Misconfiguration | Yes: weak or default secrets, permissive CORS with credentials, missing security headers, version disclosure, verbose errors. | FR-003, FR-014, FR-033–FR-036 |
| API9 Improper Inventory Management | Yes: undocumented or debug routes. | FR-038 |
| API10 Unsafe Consumption of APIs | No: the API consumes no third-party API. | — |
| A03:2025 Software Supply Chain Failures | Yes: vulnerable dependencies. | FR-041 |
| A05:2025 Injection | Yes: SQL and log injection. SQL is covered by bound parameters (AGENTS §4) and literal `q` matching (Spec 002 §5.6). | FR-040 |
| A09:2025 Security Logging and Alerting Failures | Yes: abuse must be investigable. Alerting is out of scope. | FR-039, FR-040 |
| A10:2025 Mishandling of Exceptional Conditions | Yes: a failing security check must not let a request through. | FR-036 |
| Volumetric network DDoS (botnets, L3/L4 floods) | Yes, but cannot be stopped by the API process. | Out of scope (deployment edge) |

## User Stories

| ID | Priority | Actor | Story |
|---|---|---|---|
| US1 | P1 | Admin | I log in with a username and password, receive a time-limited token, and use it to create, update, and delete resources. |
| US2 | P1 | Operator | I create, disable, and reset admin accounts through a documented procedure. No account or credential exists until I create it. |
| US3 | P1 | Student | I keep fetching without any key. Normal classroom use never hits a limit; if I exceed one, the error tells me what happened and when to retry. |
| US4 | P1 | Operator | A single client (bot, scraper, runaway loop, slow-connection attack) cannot exhaust the API for everyone else. |
| US5 | P2 | Admin | Password-guessing against the login is throttled, and the login never reveals which usernames exist. |
| US6 | P2 | Operator | Security events are logged without secrets so I can investigate abuse. |
| US7 | P2 | Developer | Every response follows safe defaults (headers, CORS, no fingerprinting, no internal details) and every route is documented. |

## Requirements

### Accounts

- FR-001: An account has a username, a password, a role (`admin` or none), and a status (`active` or `disabled`). Usernames follow the slug format of Spec 002 §4.2 and are unique. Passwords are stored only as a salted, slow, memory-hard hash (AGENTS §2). No response, log, or error ever contains a password or hash.
- FR-002: Accounts are managed only by a documented operator procedure run outside the HTTP API: create an account, set its password, grant or remove the `admin` role, disable or enable it, and list accounts. No HTTP endpoint reads or changes accounts. Removing the `admin` role revokes write access while keeping the account; such an account can still log in, and its writes return `403`.
- FR-003: No account and no credential exists by default. Nothing in the repository, image, or migrations creates one.
- FR-004: Passwords are 12–128 Unicode characters. No composition rules. A password equal to the username is rejected.
- FR-005: Disabling an account, changing its password, or removing its `admin` role takes effect on the account's next request: tokens issued before the change no longer grant what the account has lost.
- FR-006: Accounts are persisted by versioned migrations with database-level integrity (Spec 001 FR-005, FR-010; Constitution V): unique username, role and status limited to their allowed values.

### Login

- FR-007: `POST /api/v1/auth/login` with body `{ "username": "...", "password": "..." }` returns `200` and `{ "data": { "access_token": "...", "token_type": "Bearer", "expires_in": 3600 } }`. The response has `Cache-Control: no-store`.
- FR-008: An unknown username, a wrong password, and a disabled account return the same `401 INVALID_CREDENTIALS` response: identical status, body, and headers. An unknown username costs the same hashing work as a known one, so response time does not reveal whether it exists.
- FR-009: The login body follows Spec 002 §4.10 body rules (`415`, `413`, `400`, unknown fields → `422`). A missing or empty field, or a password longer than 128 characters, returns `422 VALIDATION_FAILED` before any password hashing.
- FR-010: Login is the only authentication endpoint. There is no signup, logout, token refresh, or password reset over HTTP. A token stops working when it expires or when FR-005 applies.

### Tokens

- FR-011: The access token is a signed JWT (AGENTS §2) identifying the account, the issuer, the audience, the issue time, and the expiry. It is valid for 60 minutes. It carries no password, hash, or personal data.
- FR-012: The API reads a token only from the `Authorization: Bearer <token>` header. A token in the query string, a cookie, or the body is ignored.
- FR-013: A token is rejected when it is malformed; its signature does not verify; it uses any algorithm other than the single configured one (including unsigned tokens); it lacks a required claim; it is expired or not yet valid beyond a 60-second clock tolerance; its issuer or audience does not match; or its account no longer exists, is disabled, or changed its password after the token was issued.
- FR-014: The signing secret is read from the environment. The API refuses to start, exits non-zero, and prints a readable error that never contains the secret when the secret is missing, shorter than 32 bytes, or equal to a documented placeholder. Changing the secret invalidates every issued token.

### Write authorization

- FR-015: Deny by default. Every request that is not `GET`, `HEAD`, `OPTIONS`, or the login route requires a valid token (FR-013) of an `active` account. A missing, malformed, or rejected token returns `401 UNAUTHORIZED` with a `WWW-Authenticate: Bearer` header. A valid token whose account has no `admin` role returns `403 FORBIDDEN`. Routes added later inherit this rule without opting in.
- FR-016: The test-only credential of Spec 002 §4.4 is retired. No bypass of FR-015 exists in any build.
- FR-017: `GET`, `HEAD`, and `OPTIONS` ignore the `Authorization` header. A missing, invalid, or expired token never changes a read response.
- FR-018: `401` messages say in plain words whether the token is missing, expired, or invalid, and how to fix it (log in again). They do not say which verification step failed.

### Rate limiting

- FR-019: Limits are counted per client. The client is the connecting address, or the forwarded client address only when the connection comes from a configured trusted proxy. Forwarding headers from any other peer are ignored. IPv6 clients are grouped by their /64 prefix.
- FR-020: Every limit is configurable by the operator through the environment, with the documented defaults below.
- FR-021: General limit: every request that reaches it (§Check Order step 3) counts. Default: 600 requests per minute per client, with short bursts allowed. The default is sized for a classroom of about 30 students sharing one address.
- FR-022: Login limits, in addition to FR-021: at most 10 login attempts per client per 15 minutes, and at most 20 failed attempts per username per 15 minutes across all clients. When the username limit is reached, every login for that username returns `429` until the window ends, even with the correct password. Unknown and existing usernames are counted and answered identically.
- FR-023: A request over a limit returns `429 RATE_LIMITED` with a `Retry-After` header in seconds and a message stating the limit and when to retry. Rejected requests do not extend the waiting time.
- FR-024: Every response to a request that reaches the general limit check (§Check Order step 3), including `429`, states the client's limit, remaining requests, and seconds until reset in response headers. Browser scripts can read these headers and `Retry-After` (CORS exposed).
- FR-025: The limit check runs before any other processing and a rejected request does no database work.
- FR-026: With rate limiting enabled, read endpoints still meet the Constitution III latency target.
- FR-027: Clients are never blocked or treated differently by `User-Agent` or other client-declared headers. Any client, bot or not, gets data within the limits.

### Resource consumption

Spec 002 caps remain (64 KB body, `limit ≤ 100`, `q` ≤ 64 characters, list maxima).

- FR-028: A URL longer than 2048 characters returns `414 URI_TOO_LONG`. Request headers larger than 16 KB in total return `431 REQUEST_HEADERS_TOO_LARGE`.
- FR-029: A client that does not send complete request headers within 10 seconds, or a complete body within 30 seconds, gets `408 REQUEST_TIMEOUT` or is disconnected. Idle connections are closed after a bounded time.
- FR-030: The API bounds the number of requests it processes at once. Above that bound, extra requests get `503 SERVICE_UNAVAILABLE` with `Retry-After` within 1 second instead of waiting in an unbounded queue.
- FR-031: Every database operation has a time limit. Exceeding it returns `503 SERVICE_UNAVAILABLE` with a generic message.
- FR-032: The API bounds the number of password verifications running at once. Above that bound, a login returns `503 SERVICE_UNAVAILABLE` with `Retry-After`.

### Hardening

- FR-033: Every response the application produces, including `304`, errors, and `429`, carries `X-Content-Type-Options: nosniff`, `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`, `Referrer-Policy: no-referrer`, and `Strict-Transport-Security`. No header reveals the server software or version.
- FR-034: CORS, extending Spec 002 §4.3: `Access-Control-Allow-Origin: *` on every route. Preflight allows the `Authorization` and `Content-Type` request headers and the `GET`, `POST`, `PATCH`, and `DELETE` methods. `Access-Control-Allow-Credentials` is never sent. Because tokens travel only in a header (FR-012), a browser never sends them automatically, so cross-site request forgery is not possible.
- FR-035: Login responses and responses to authenticated writes carry `Cache-Control: no-store`.
- FR-036: Fail closed. If a security check (token verification, account lookup, rate limit) cannot complete, the request is denied with a generic `500` or `503`, never let through. Every error response the application produces, including `408`, `414`, `429`, `431`, and `503`, uses the Spec 002 error envelope and contains no internal details.
- FR-037: The API never fetches `image_url` or any other client-supplied URL.
- FR-038: Every reachable route, including login, is in the OpenAPI description with its security requirement and its `401`, `403`, and `429` responses. No undocumented route (debug, test, metrics) is reachable in a built API.

### Logging

- FR-039: The API logs these security events with time, client, account (when known), route, and outcome: login success, login failure, login lockout, `401` and `403` on writes, `429` (at most once per client per window), `503` from FR-030–FR-032, every successful admin write (method, path, resource id), and startup refusals from FR-014.
- FR-040: Logs never contain passwords, tokens, `Authorization` headers, password hashes, the signing secret, or database credentials. Client-supplied values in logs are escaped so they cannot forge or split log entries.

### Supply chain

- FR-041: Dependencies are checked against a public known-vulnerability database as part of the quality gates (AGENTS §5). A known vulnerability in a dependency fails the gate unless the plan records why it does not apply.

## Check Order

Extends Spec 002 §4.10 "Check order". These steps run before Spec 002 step 1; the first applicable response wins.

1. `408`, `414`, `431`: connection and request-size limits (FR-028, FR-029)
2. `503`: processing capacity (FR-030)
3. `429`: general rate limit (FR-021)
4. Spec 002 steps 1–6, where step 2 (`401` / `403`) is FR-015

Login route, after steps 1–3: `429` per-client login limit (FR-022) → `413` / `415` / `400` body → `422` → `429` per-username login limit (FR-022) → `503` hashing capacity (FR-032) → `401 INVALID_CREDENTIALS` → `200`.

## Error Codes

Added to Spec 002 §4.12.

| Code | Status |
|---|---|
| `INVALID_CREDENTIALS` | 401 |
| `REQUEST_TIMEOUT` | 408 |
| `URI_TOO_LONG` | 414 |
| `RATE_LIMITED` | 429 |
| `REQUEST_HEADERS_TOO_LARGE` | 431 |
| `SERVICE_UNAVAILABLE` | 503 |

## Key Entities

- **Account**: username, password hash, role, status, time of the last credential change. Never exposed over HTTP.
- **Access token**: signed, short-lived proof of an account's identity. Not stored by the API.
- **Rate-limit counter**: requests per client (and login failures per username) within a window. Short-lived abuse-control state; losing it only resets the windows.

## Acceptance Criteria

1. On a fresh database no login succeeds. After the operator creates an admin through the documented procedure, login returns `200`, `token_type` `Bearer`, `expires_in` `3600`, and `Cache-Control: no-store`. (US1, US2)
2. A write with that token succeeds as defined in Spec 002 and carries `Cache-Control: no-store`. The same write without `Authorization` returns `401 UNAUTHORIZED` with `WWW-Authenticate: Bearer`. A write with a valid token of an account without the `admin` role returns `403 FORBIDDEN`. (US1)
3. A write returns `401` with a token that has an altered payload, a removed signature, `alg` set to `none`, a different algorithm, a different secret, the wrong issuer or audience, no expiry, or an expiry more than 60 seconds past. (US1)
4. A write with a valid token sent only in the query string, a cookie, or the body returns `401`. (US1)
5. After the account is disabled or its password changes, its earlier token returns `401` on the next write. After its admin role is removed, its earlier token returns `403`. (US2)
6. Login with an unknown username, a wrong password, and a disabled account return byte-identical status, body, and headers. Over 50 attempts each, the median duration of unknown-username logins is within 25% of wrong-password logins. (US5)
7. An account cannot be given an 11-character or 129-character password, or one equal to its username. A login with a 129-character password returns `422` without hashing. (US2)
8. A `GET` with a garbage or expired `Authorization` header returns the same status, body, and headers as without it. (US3)
9. The API refuses to start, exits non-zero, and does not print the secret when the signing secret is missing, shorter than 32 bytes, or a documented placeholder. (US2)
10. An automated inventory of every registered route shows that each route other than `GET`, `HEAD`, `OPTIONS`, and login returns `401` without a token, and that the registered routes and the OpenAPI routes are the same set. (US7)
11. A client that exceeds the general limit gets `429 RATE_LIMITED` with `Retry-After`. After waiting that many seconds, its requests succeed. Another client is never affected. (US3, US4)
12. A client sending requests at the default general limit for 10 minutes gets no `429`, and read p95 stays below 50 ms at the server. (US3)
13. A forwarding header sent by an untrusted peer does not change the counted client. Sent by a configured trusted proxy, it does. Two IPv6 addresses in the same /64 share one limit. (US4)
14. The 11th login attempt from one client within 15 minutes returns `429`, even with correct credentials. After 20 failed logins for one username from different clients, the next login for that username returns `429`; an unknown username behaves identically. (US5)
15. Every response that reaches the general limit check, including `304` and `429`, has the rate-limit headers, and a browser script on another origin can read them and `Retry-After`. (US3)
16. A URL of 2049 characters returns `414`. Headers of more than 16 KB return `431`. A client sending its headers one byte per second is cut off within 10 seconds. A client that sends complete headers but not its full body within 30 seconds gets `408` or is disconnected. (US4)
17. With processing at capacity, an extra request gets `503` with `Retry-After` within 1 second, and service returns to normal when load drops. A database operation over its time limit returns `503` with no SQL in the body. (US4)
18. Every response the application produces, including `304`, `4xx`, `429`, and `5xx`, carries the FR-033 headers and no server version header. (US7)
19. A preflight for `POST` with `Authorization` from any origin succeeds. No response ever contains `Access-Control-Allow-Credentials`. (US7)
20. When token verification, the account lookup, or the rate-limit check fails internally, the request is denied, never served. (US4)
21. After a run that logs in, writes, fails a login, reaches a login lockout, gets a `403`, and gets a `429`, the log has an event of each kind, and searching the logs for the password, the token, and the signing secret finds nothing. A username containing a line break produces one log entry, not two. (US6)
22. Creating a species whose `image_url` points to a test listener causes no request to that listener. (US7)
23. The dependency vulnerability check runs in the quality gates and fails on a dependency with a known vulnerability. (US7)
24. Creating an account whose username is taken or is not a valid slug fails, and the database itself rejects a duplicate username and a role or status outside the allowed values. (US2)
25. `401` responses for a missing, an expired, and a tampered token have different messages, each telling the client how to fix it, and none names the failed verification step. (US1)
26. With the general and login limits set through the environment to values other than the defaults, the API enforces the configured values. (US4)
27. A request rejected with `429` causes no database query. (US4)
28. Requests that differ only in `User-Agent` (a browser, a known bot, or none) get the same responses and share one limit. (US3)
29. With password verifications at their bound, an extra login with a well-formed body returns `503` with `Retry-After`. (US5)
30. Every `408`, `414`, `429`, `431`, and `503` response the application produces has a body that uses the Spec 002 error envelope with its code from §Error Codes and no internal details. (US7)

## Constitution Check

| Principle | Compliance |
|---|---|
| I. Easy to Fetch | ✅ Reads need nothing and ignore `Authorization` (FR-017). Limits are generous, documented, and readable from scripts (FR-024). `429` says when to retry. |
| II. Simple CRUD, Restricted Writes | ✅ Fulfilled: deny-by-default admin check on every write (FR-015). |
| III. High Performance | ✅ Rejections do no database work (FR-025); the read target holds with limits on (FR-026, AC 12). |
| IV. Test-First | ✅ Every FR maps to an acceptance criterion testable against the real database. |
| V. Data Integrity | ✅ Account rules enforced by the database (FR-006). |
| VI. Simplicity | ✅ One login endpoint; no refresh tokens, signup, MFA, or revocation lists (FR-010). Rate-limit and login-attempt counters and concurrency permits are short-lived abuse-control state; keeping them in process memory complies with constitution 1.2.0 Principle VI and is not a deviation. |

## Out of Scope

- Volumetric DDoS, WAF, CDN, and IP allow/deny lists: handled at the deployment edge.
- TLS termination: credentials travel only over HTTPS in deployment; the deployment edge provides it.
- Signup, password reset, logout, refresh tokens, token revocation lists, MFA, SSO/OAuth, API keys for readers, CAPTCHA.
- Alerting and monitoring dashboards.
- An audit table of admin changes in the database (logs only, FR-039).
- Signing-key rotation without invalidating issued tokens.

## Assumptions

- In production a TLS-terminating proxy sits in front of the API, and the operator configures it as a trusted proxy (FR-019).
- Admin accounts are few (fewer than 10) and writes are rare.
- A 60-minute token without refresh is acceptable for admin curation sessions.
- Server clocks are synchronized; the 60-second tolerance covers residual skew.
- The per-username login limit lets an attacker delay a specific admin's login by up to 15 minutes; this trade-off is accepted over allowing unlimited guessing.
