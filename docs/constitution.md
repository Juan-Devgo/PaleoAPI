# PaleoAPI Constitution

**Version:** 1.1.0
**Ratified:** 2026-09-26
**Last amended:** 2026-09-26

This document defines the non-negotiable principles of PaleoAPI. Every specification, plan, task, and line of code MUST comply with it. When a spec or implementation conflicts with this constitution, the constitution wins.

---

## 1. Mission

PaleoAPI is a public, high-performance REST API that serves data about ancient species that lived on Earth across hundreds of millions of years.

It exists to be:

1. **A learning resource** for students practicing how to fetch data from an API and display it in a frontend.
2. **A sandbox for examples and proofs of concept**, where developers need realistic, rich, and reliable data without setup friction.

Because of this, **ease of consumption is a first-class requirement**. Any decision that makes the API harder to fetch, understand, or display must be justified against this mission.

---

## 2. Core Principles

### I. Easy to Fetch

- Read endpoints are **public**: no API key, no signup, no auth header.
- CORS is open to all origins for `GET` requests.
- Responses are plain JSON with predictable shapes, `snake_case` field names, and no surprises between endpoints.
- A student must be able to get useful data with a single `fetch("https://.../api/v1/species")` call.
- Error messages are human-readable and explain what went wrong and how to fix it.

### II. Simple CRUD, Restricted Writes

- The domain is a CRUD system. No feature may introduce complexity beyond what the mission requires.
- **Everyone can read.** Only users with the `admin` role can create, update, or delete.
- Write operations are never exposed without authentication and role checks.

### III. High Performance

- Read endpoints target **p95 latency < 50 ms** measured at the server (excluding network).
- Every `GET` response includes HTTP caching headers (`ETag` and/or `Cache-Control`). Conditional requests (`If-None-Match`) return `304 Not Modified`.
- Every query used by a public endpoint MUST be backed by appropriate indexes.
- No N+1 queries. Related data is loaded with joins or batched queries.
- List endpoints are always paginated; no endpoint returns an unbounded collection.

### IV. Test-First (NON-NEGOTIABLE)

- Tests are written from the spec's acceptance criteria **before** the implementation.
- Every feature requires:
  - **Unit tests** for domain logic and validation.
  - **Integration tests** against a real PostgreSQL instance (Docker/testcontainers). Mocking the database for integration tests is not allowed.
- A feature is not done until all tests pass, `cargo clippy -- -D warnings` is clean, and `cargo fmt --check` passes.

### V. Data Integrity & Scientific Accuracy

- The database enforces integrity: foreign keys, `NOT NULL`, `UNIQUE`, and `CHECK` constraints are mandatory where the domain requires them. Validation in Rust complements, never replaces, DB constraints.
- Data should be scientifically plausible and consistent (e.g., `start_mya > end_mya`, `min_* <= max_*`, a period belongs to exactly one era).
- Taxonomy follows the standard hierarchy: Domain → Kingdom → Phylum → Class → Order → Family → Genus → Species.

### VI. Simplicity

- Prefer the simplest solution that satisfies the spec (YAGNI).
- No new dependency, service, or abstraction layer without a documented reason in the plan.
- Stateless API process: all state lives in PostgreSQL.
