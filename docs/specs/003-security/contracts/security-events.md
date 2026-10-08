# Contract — Security event log (FR-039, FR-040, research R17)

One JSON object per line on standard output. Spec 002's internal-error lines stay on standard error.

## Fields

| Field | Type | Present | Value |
|---|---|---|---|
| `ts` | string | always | RFC 3339 UTC with milliseconds, `2026-10-07T14:03:22.418Z` |
| `event` | string | always | see below |
| `client` | string | request events | resolved client (research R5): an IPv4 address, or an IPv6 /64 as `2001:db8:1:2::/64` |
| `account` | string | when it names an existing account | username |
| `method` | string | request events | HTTP method |
| `route` | string | request events | matched pattern (`/api/v1/species/{species_id}`), else `unmatched` |
| `path` | string | `admin_write` only | request path as received |
| `resource` | string | `admin_write` only | id of the created/changed/deleted resource |
| `status` | integer | request events | response status |
| `reason` | string | see below | machine-readable cause |
| `count` | integer | `overloaded` | events of this reason in the last second (≥ 1) |

## Events

| `event` | When | `reason` |
|---|---|---|
| `login_succeeded` | login `200` | — |
| `login_failed` | login `401` | — |
| `login_locked` | a failure fills the per-username log | — |
| `auth_rejected` | write `401` | `missing`, `expired`, `invalid` |
| `forbidden` | write `403` | — |
| `rate_limited` | `429`, at most once per client per window | `general`, `login_client`, `login_username` |
| `overloaded` | `503` from FR-030–FR-032, at most one line per reason per second | `capacity`, `hashing`, `database`, `limiter` |
| `admin_write` | successful (`2xx`) write | — |
| `startup_refused` | FR-014 refusal: `JWT_SECRET` missing, short, or placeholder (also printed to stderr); other startup failures are only printed | `jwt_secret_missing`, `jwt_secret_short`, `jwt_secret_placeholder` |

- `resource`: the last segment of `Location` for `201`, otherwise the last path segment.
- Never logged (FR-040): passwords, tokens, `Authorization`, cookies, hashes, `JWT_SECRET`, `DATABASE_URL`, request bodies, and usernames that do not name an existing account.
- Every string goes through JSON string escaping, so control characters (including line breaks) are escaped and a value cannot start a new line.

Example:

```json
{"ts":"2026-10-07T14:03:22.418Z","event":"auth_rejected","client":"203.0.113.7","method":"PATCH","route":"/api/v1/species/{species_id}","status":401,"reason":"expired"}
```
