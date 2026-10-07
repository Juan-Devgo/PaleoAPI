# Spec 001 — Database Foundation

**Status:** Clarified (pending approval) · **Updated:** 2026-10-03 · **Constitution:** 1.1.0 · **Required by:** [Spec 002](../002-crud/spec.md)

Resources, fields, and domain rules are defined in Spec 002 §4–§5. This spec requires that they be persisted and enforced by the database; it does not redefine them. On any apparent conflict, Spec 002 wins.

## Clarifications

- 2026-10-02: The API never applies migrations; it refuses to start while any are pending (FR-004).
- 2026-10-02: The database rejects a `discovery_year` after the current UTC year (§Integrity).
- 2026-10-02: The database enforces list minimums only; maximums are API-only request-size limits (§Integrity, §Constitution Check).
- 2026-10-02: Index verification uses 10,000 species with proportional eras, periods, taxonomy, and geography (AC 14).
- 2026-10-03: Case-insensitive name sorts use a linguistic order that is stable across OS upgrades (FR-016, AC 18).
- 2026-10-03: Species size measures have no upper bound.
- 2026-10-03: AC 14 requires each public query to be *servable* by an index, verified with the query planner, not "never a full scan".
- 2026-10-03: Spec 002 `q` is 3–64 characters, so every term with 3 consecutive letters or digits is index-selective (FR-011).
- 2026-10-03: A `q` term without 3 consecutive letters or digits (e.g. `%%_`) is an accepted non-selective edge case; it stays within the Constitution III target on the representative dataset (FR-011).
- 2026-10-03: Running tests against the development database by mistake is not prevented in the database; it is detected, fails loudly, never touches resource data, and has a documented cleanup step (FR-015, AC 13).

## Motivation

- Constitution IV: integration tests need a real PostgreSQL; no API feature can finish without it.
- Constitution V and Spec 002 §7: the database itself must guarantee that no sequence of writes leaves the data inconsistent.
- The draft schema in the repository conflicts with Spec 002 (types, lengths, missing hierarchy and species–country links, uppercase enums, cascading deletes, no range/overlap/geography rules, out-of-scope user tables). It is replaced, not patched (FR-007).

## User Stories

| ID | Priority | Actor | Story |
|---|---|---|---|
| US1 | P1 | Contributor | I start a local database with one documented command and build the full schema with one documented command, with no manual SQL. |
| US2 | P1 | Admin | No sequence of writes, including concurrent ones, can leave the dataset inconsistent, even if an application bug slips through. |
| US3 | P1 | Contributor | I run the full test suite against a real database; tests do not interfere with each other or with my development data. |
| US4 | P2 | Student | Every public list, filter, sort, and search is fast because each one is backed by an index. |
| US5 | P2 | Operator | I configure the database connection only through the environment, and the API refuses to start when the database is not usable. |

## Requirements

### Environment and configuration

- FR-001: A contributor can start a local database with a single documented command.
- FR-002: The database connection is configured only through environment variables. No credentials are committed; a documented example with placeholder values may be.
- FR-003: If the database cannot be reached within a bounded startup wait, the API exits non-zero with a human-readable error and does not serve requests. The error never includes connection credentials.
- FR-004: The API never changes the schema. Migrations are applied only by a separate, explicit, documented step. With migrations pending, the API refuses to start, exits non-zero, and names the problem in a human-readable error.

### Schema versioning

- FR-005: Every schema change is a versioned migration in the repository. Migrations are the only source of the schema.
- FR-006: Applying all migrations to an empty database produces the complete schema with no errors. Applying them again changes nothing.
- FR-007: The existing draft schema file is removed.

### Data model

- FR-008: The schema stores every resource and relationship in Spec 002 §5: eras; periods with their era; the seven taxonomy ranks with their parent links; continents; countries with their continent links; species with their genus, optional size, periods, continents, and countries.
- FR-009: Decimal values (`start_mya`, `end_mya`, size bounds) are stored exactly as sent within their allowed precision, never as whole numbers or floating-point values.
- FR-010: Users, roles, and credentials are not part of this schema (security spec).

### Integrity enforced by the database

Each rule is rejected by the database itself, independently of API validation. A rejected write leaves stored data unchanged.

| Area | Rules |
|---|---|
| Identifiers | Every `id` is unique within its resource type, matches the slug format (Spec 002 §4.2), and never changes once stored. |
| Text | Stored text has no leading or trailing whitespace and is not empty (Spec 002 §4.13). Lengths count Unicode characters. |
| Names | Every `name` is required, 1–64 characters, and unique within its resource type ignoring case, except species `name`, which is not unique. Species `scientific_name` is 1–128 characters and unique ignoring case. Renaming a row to a different casing of its own name is allowed. |
| Enums | `diet`: `carnivore`, `herbivore`, `omnivore`, `piscivore`, `insectivore`. Continent `type`: `prehistoric`, `modern`. Lowercase only. |
| Time ranges | Eras and periods: `start_mya > end_mya`; both within 0–4600; at most 3 decimal places. More decimal places are rejected, never rounded. |
| Era overlaps | Eras do not overlap. Touching boundaries and gaps are allowed. |
| Period overlaps | Periods of the same era do not overlap. Touching boundaries are allowed. |
| Containment | A period's range lies within its era's range (boundaries may coincide), when the period changes, when it moves to another era, and when the era's range changes. |
| Hierarchy | Every period has exactly one era. Every taxonomy rank except domain has exactly one parent from the rank directly above. Every species has exactly one genus. |
| Geography | Every continent linked to a country is `modern`, when the link is added and when a linked continent's `type` changes. A species may link to continents of either type. |
| Species text | `description` is 1–1000 characters. `image_url`, when present, is at most 2048 characters and starts with `http://` or `https://`. |
| Species size | Optional. When present it has at least one of length, height, weight. Each present measure has both `min` and `max`, each `> 0` with at most 6 decimal places (rejected, never rounded), and `min ≤ max`. |
| Discovery year | `discovery_year`, when present, is `≥ 1600` and not after the current UTC year at write time. |
| List cardinality | A country has at least one continent link; a species has at least one period link, on create and when links are removed. The check applies to the committed result, so a row and its first link can be written together. Maximum list lengths are API-only (§7). |
| References | Every reference points to an existing row. A link between the same two rows is stored at most once. |
| Deletes | Deleting a row that others depend on is rejected; no cascading deletes, except rows owned only by the deleted resource: a country's continent links, and a species' size and its period, continent, and country links. |
| Concurrency | All rules hold under concurrent writes. Two writes that are each valid alone but conflict together (e.g. two overlapping eras) cannot both be stored. |

### Indexes

- FR-011: Every filter, sort, lookup, and nested listing that Spec 002 exposes publicly is backed by an index (Constitution III), including the case-insensitive substring search `q` on species `name` and `scientific_name`, and the case-insensitive name sorts.
- FR-016: Case-insensitive name sorts use a linguistic order in which accented letters sort with their base letter. The order, and its indexes, stay valid when the host OS is upgraded.
- FR-012: Every lookup the database performs to enforce the integrity rules (references, delete protection, overlaps, containment) is backed by an index.

### Tests

- FR-013: Integration tests run against a real database with all migrations applied.
- FR-014: Tests are isolated (no test sees another's data) and can run in parallel.
- FR-015: The documented test command never reads or changes the development database's data. If tests run against the development database by mistake, the run fails with a clear message, no resource data is read or changed, and the documentation gives the cleanup step for the harness's bookkeeping records.

## Acceptance Criteria

1. On a clean checkout with the environment configured, starting the database and building the schema take one documented command each and produce the full schema with no errors and no manual SQL. Building again reports nothing to apply and leaves the schema unchanged. (US1)
2. With the database unavailable, the connection configuration missing, or migrations pending, starting the API exits non-zero (within the bounded wait when unreachable) with a readable error containing no credentials. The API never applies migrations. (US5)
3. For each integrity rule, an integration test writing directly to the database shows the invalid write is rejected and stored data is unchanged; a matching valid write succeeds. (US2)
4. Eras 251.902–66.0 and 66.0–0 can both be stored. Eras 251.902–66.0 and 70.0–0 cannot. (US2)
5. Shrinking an era so one of its periods falls outside it is rejected. Moving a period to an era whose range does not contain it is rejected. (US2)
6. A `start_mya` of 66.0001 or a size bound with 7 decimal places is rejected, not rounded. (US2)
7. A slug `id` that breaks the format is rejected. Changing a stored `id` is rejected. (US2)
8. A second era named `MESOZOIC` when `Mesozoic` exists is rejected. Two species with the same `name` and different `scientific_name` are both stored. Renaming `Mesozoic` to `MESOZOIC` succeeds. (US2)
9. Linking a country to a `prehistoric` continent is rejected. Changing a continent that has countries from `modern` to `prehistoric` is rejected. (US2)
10. Deleting an era with periods, a period linked to a species, a taxonomy item with children, a genus with species, a continent linked to a country or species, or a country linked to a species is rejected. Deleting a country with no species removes its continent links. Deleting a species removes its size and links. (US2)
11. Two concurrent transactions that each insert one of two overlapping eras cannot both commit; likewise two overlapping periods of one era. (US2)
12. Two integration tests that create a resource with the same `id` can run in parallel without failing. (US3)
13. After a full run of the documented test command, the development database's data is identical to before. Running the tests against the development database by mistake fails the run with a clear message. (US3)
14. Each public query in Spec 002 can be served by an index, verified with the database's query planner on 10,000 species with proportional eras, periods, taxonomy, and geography. (US4)
15. The repository contains no database credentials other than documented placeholders. (US5)
16. A `discovery_year` after the current UTC year is rejected by the database on insert and on update. (US2)
17. A species with no period link, or a country with no continent link, cannot be committed, including when its last link is removed and when two concurrent transactions each remove one of its last two links. (US2)
18. Sorting species by name ignoring case places "Ñandú" between "Nandu" and "Oviraptor", not after "Zebra". (US4)

## Constitution Check

PASS. One deviation:

| Rule | Enforced by | Justification |
|---|---|---|
| Maximum list lengths (Spec 002: country continents ≤ 10; species periods ≤ 20, continents ≤ 20, countries ≤ 50) | API only | Request-size limits, not domain facts (Clarifications 2026-10-02). Domain minimums stay in the database. |

## Out of Scope

- API endpoints and API-level validation messages (Spec 002).
- Users, roles, and credentials tables (security spec).
- Seed data and bulk import.
- Backups, replication, and production hosting.
