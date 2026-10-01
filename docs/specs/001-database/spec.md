# Spec 001 — Database Foundation

**Status:** Draft
**Created:** 2026-09-26
**Constitution version:** 1.1.0
**Required by:** [Spec 002 — Core CRUD](../002-crud/spec.md)

---

## 1. Summary

Provide the persistent storage that every PaleoAPI feature builds on: a PostgreSQL database available in development and tests, a versioned migration workflow, and a schema that stores every resource defined in Spec 002 §5 with its integrity rules enforced by the database itself.

The resources, fields, and domain rules are defined once, in Spec 002 §5. This spec requires that they be persisted and enforced at the database level. It does not redefine them.

## 2. Motivation

- Constitution IV requires integration tests against a real PostgreSQL instance. No API feature can be finished until that database exists.
- Constitution V requires the database, not only Rust code, to guarantee integrity.
- The current draft `src/db/shemas_definition.sql` does not match the API contract (see §5). It must be replaced before any CRUD work starts.

## 3. Users & User Stories

| Actor | Story |
|---|---|
| Contributor | As a contributor, I start a local database with one command and apply the schema with one command, with no manual SQL. |
| Contributor | As a contributor, I run the full test suite against a real PostgreSQL, and tests do not interfere with each other or with my development data. |
| Admin | As an admin, I trust that no sequence of writes can leave the dataset inconsistent, even if an application bug slips through. |
| Operator | As an operator, I configure the database connection only through environment variables. |

## 4. Requirements

### 4.1 Environment

- A PostgreSQL service starts with `docker compose up -d db`. Its version is [NEEDS CLARIFICATION: PostgreSQL major version].
- The connection is configured only through environment variables (for example `DATABASE_URL`). No credentials are committed. An example file with placeholder values may be committed.
- If the database is unreachable at startup, the API exits with a clear error message and a non-zero exit code. It does not start serving requests. [NEEDS CLARIFICATION: should the API also apply pending migrations at startup, or only via `sqlx migrate run`?]

### 4.2 Migrations

- Every schema change is a versioned migration in `migrations/`, applied with `sqlx migrate run`.
- Applying all migrations to an empty database produces the complete schema. Applying them again changes nothing.
- The draft `src/db/shemas_definition.sql` is removed. Migrations are the only source of the schema.

### 4.3 Data model

The schema stores every resource and relationship in Spec 002 §5: eras, periods, the seven taxonomy ranks with their parent links, continents, countries with their continent links, and species with their size, genus, periods, continents, and countries.

### 4.4 Integrity enforced by the database

Each rule below must be rejected by the database itself, independently of API validation:

| Area | Rules |
|---|---|
| Identifiers | `id` is the primary key and matches the slug format (Spec 002 §4.2). |
| Names | Required, 1–64 characters, unique per resource type ignoring case. Species `name` is not unique. Species `scientific_name` is unique ignoring case. |
| Enums | `diet` and continent `type` accept only their lowercase values. |
| Time ranges | `start_mya > end_mya`, both within `0–4600` with at most 3 decimal places. |
| Overlaps | Eras do not overlap. Periods of the same era do not overlap. Touching boundaries are allowed. |
| Containment | A period's range lies within its era's range. This holds when either the period or the era changes. |
| Hierarchy | Every period has exactly one era. Every rank except domain has exactly one parent of the rank directly above. Every species has exactly one genus. |
| Geography | A country has at least one continent link [NEEDS CLARIFICATION: enforce "at least one" in the database, or only in the API?]. Every continent linked to a country is `modern`. |
| Species | At least one period. Size bounds are `> 0` with `min ≤ max`. `discovery_year ≥ 1600`. Text length limits from Spec 002 §5.6. |
| References | Every reference points to an existing row. No duplicate links. |
| Deletes | Deleting a row that others depend on is rejected (no cascades), except for rows owned only by the deleted resource: a country's continent links and a species' size and links are removed with it. |

**Justified exception:** the upper bound of `discovery_year` ("not after the current year") depends on the clock, so the database cannot express it as a constraint. It is enforced by the API only.

### 4.5 Indexes

Every filter, sort, and lookup that Spec 002 exposes on a public endpoint is backed by an index, including the case-insensitive `q` substring search on species names. This is required to meet the Constitution III p95 < 50 ms target.

### 4.6 Tests

- Integration tests run against a real PostgreSQL instance with all migrations applied. The database is never mocked.
- Tests are isolated: one test's data is never visible to another, and tests can run in parallel.
- Running the tests never touches the development database's data.

## 5. Known Problems in the Current Draft

`src/db/shemas_definition.sql` conflicts with Spec 002: it uses 8-character ids, integer `mya` values and sizes, a flat taxonomy table with no rank parent links, uppercase enums, and `ON DELETE CASCADE` / `SET NULL` where Spec 002 requires delete protection. It also has typos (`max_leght_m`, a wrong `CHECK` on that column, `DROP TABLE peroid`). It is replaced, not patched.

## 6. Acceptance Criteria

1. On a clean checkout with the environment configured, `docker compose up -d db` followed by `sqlx migrate run` produces the full schema with no errors. Running `sqlx migrate run` again changes nothing.
2. With the database stopped, starting the API exits non-zero with a readable error message.
3. For each rule in §4.4, an integration test that bypasses the API and writes directly to the database shows the invalid write is rejected.
4. Eras 251.902–66.0 and 66.0–0 can both be stored. Eras 251.902–66.0 and 70.0–0 cannot.
5. Shrinking an era so that one of its periods falls outside it is rejected by the database.
6. Deleting an era with periods, a genus with species, or a country linked to a species is rejected. Deleting a species removes its size and links.
7. Each public query in Spec 002 uses an index on a representative dataset. [NEEDS CLARIFICATION: dataset size to verify against, for example 10,000 species.]
8. Two integration tests that create the same `id` can run in parallel without failing.
9. No credentials exist in the repository.

## 7. Constitution Check

| Principle | Compliance |
|---|---|
| I. Easy to Fetch | ✅ No public surface. Enables Spec 002. |
| II. Simple CRUD, Restricted Writes | ✅ Storage only. No users or roles yet (security spec). |
| III. High Performance | ✅ Every public query must be index-backed (§4.5). |
| IV. Test-First | ✅ Delivers the real PostgreSQL that integration tests require (§4.6). Database rules are tested directly (§6.3). |
| V. Data Integrity & Accuracy | ✅ Every domain rule is enforced by the database (§4.4). One justified exception: the clock-dependent `discovery_year` upper bound. |
| VI. Simplicity | ✅ PostgreSQL + `sqlx` migrations only, per the tech stack. Any database extension needed for §4.4 or §4.5 must be justified in the plan. |

**Deviations:** none. The `discovery_year` upper bound is a documented technical limit, not a deviation. Constitution V requires the database to enforce integrity "where the domain requires them", and the API still enforces this rule.

## 8. Out of Scope

- API endpoints (Spec 002).
- Users, roles, and credentials tables (security spec).
- Seed data and bulk import.
- Backups, replication, and production hosting.
