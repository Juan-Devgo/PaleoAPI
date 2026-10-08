# Requirements Checklist — Spec 003

**Date:** 2026-10-07 · **Result:** PASS (all speckit-specify quality items, after the fixes below)

- Technology named only where mandated: JWT and the memory-hard password hash (AGENTS §2), environment variables (12-factor), migrations and database integrity (Constitution V), OpenAPI (AGENTS §5). Routes, headers, status codes, and error codes are the API contract, as in Spec 002. Libraries, algorithms, storage of counters, and env var names live in the plan.
- Fixed: FR-001 and FR-011 cited a "constitution stack"; the stack is in AGENTS §2.
- Fixed: Check Order evaluated the per-username login limit (FR-022) before the body was parsed. Split into a per-client limit before the body and a per-username limit after `422`.
- Fixed: FR-021 "every request counts" and FR-024 / AC 15 "every response" conflicted with Check Order steps 1–2, which answer before the limit check. Scoped to requests that reach step 3.
- Fixed: Constitution Check IV claims every FR maps to an AC. Added AC 24–30 (FR-001/FR-006, FR-018, FR-020, FR-025, FR-027, FR-032, FR-036 envelope) and extended AC 2 (FR-035), AC 16 (FR-029 body timeout), and AC 21 (FR-039 lockout and `403`). Existing FR/AC numbers unchanged.
- Plan decisions, not ambiguities: burst size (FR-021), idle-connection timeout (FR-029), concurrency bounds (FR-030, FR-032), database time limit (FR-031), `Strict-Transport-Security` value (FR-033), `500` vs `503` per fail-closed case (FR-036), counter storage (Constitution Check VI).
- The per-client login limit runs before the body, so it counts every request to the login route (AC 14).
