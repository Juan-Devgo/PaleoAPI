# Spec 002 — Core CRUD for PaleoAPI Resources

**Status:** Draft
**Created:** 2026-09-26
**Last updated:** 2026-10-03
**Constitution version:** 1.1.0
**Depends on:** [Spec 001 — Database Foundation](../001-database/spec.md)

---

## 1. Summary

Define the public read and admin write behavior of PaleoAPI's core resources: **species**, **geologic time** (eras and periods), **taxonomy ranks** (domain → genus), and **geography** (continents and countries).

This spec defines the API contract: what each endpoint accepts, returns, and rejects. Persistence (database availability, migrations, and integrity constraints) is provided by Spec 001, which must be implemented first. Authentication is covered by the later security spec (see §9).

## Clarifications

### Session 2026-10-03

- Q: Should `q` accept 2-character terms, given that a 2-character substring search cannot be served by an index (Constitution III)? → A: No. `q` is 3–64 characters after trimming (§5 species filters, AC 8). Raised during Spec 001 analysis.

## 2. Motivation

PaleoAPI exists so students and developers can fetch realistic prehistoric data with no setup (Constitution §1). A student must be able to get a displayable species card from one `fetch` call. Admins must be able to keep the dataset accurate and consistent. Every later feature depends on this contract.

## 3. Users & User Stories

| Actor | Story |
|---|---|
| Student | As a student, I fetch `/api/v1/species` and render a list of species with name, image, diet, and period without extra calls. |
| Student | As a student, I filter species by diet, era, period, taxonomy rank, continent, or country, and search by name, to build filter UIs. |
| Student | As a student, I page through large lists using `page` and `limit` and read totals from the response to build pagination controls. |
| Developer | As a developer, I browse the taxonomy tree (domains → kingdoms → … → genera) and the geologic timeline (eras → periods) to build tree or timeline views. |
| Developer | As a developer, I get clear error messages that tell me what I did wrong and how to fix it. |
| Admin | As an admin, I create, partially update, and delete any resource, and the API rejects changes that would make the data inconsistent. |

## 4. Global Rules

These apply to every endpoint in this spec.

### 4.1 Base path and format

- All endpoints live under `/api/v1`.
- Request and response bodies are JSON. Field names are `snake_case`.
- Enumerated values (`diet`, `type`) are lowercase strings.

### 4.2 Identifiers

- Every resource has an `id` chosen by the admin on create. It is a readable slug, for example `tyrannosaurus-rex` or `jurassic`.
- Slug format: 2–64 characters; lowercase `a–z`, digits `0–9`, and hyphens; no leading or trailing hyphen; no consecutive hyphens.
- IDs are unique within their resource type and **immutable**. Sending `id` in a `PATCH` body returns `422`.
- Creating a resource with an existing `id` returns `409`.
- A path `id` that is not a valid slug returns `404 <RESOURCE>_NOT_FOUND`, the same as an unknown id.

### 4.3 Read access

- All `GET` endpoints are public: no authentication, no API key.
- Cross-origin `GET` requests are allowed from any origin (`Access-Control-Allow-Origin: *`).
- The `ETag` header is exposed to browser scripts (`Access-Control-Expose-Headers: ETag`).
- CORS preflight (`OPTIONS`) requests are answered successfully on every route and never return `405`.

### 4.4 Write access

- `POST`, `PATCH`, and `DELETE` require an admin. Missing or invalid credentials return `401`. Valid credentials without the admin role return `403`.
- Write endpoints are **deny-by-default**: every write passes through an admin check before any other processing. How credentials are issued and verified is defined by the security spec (§9). Until that spec ships, the admin check rejects every request with `401`, in every deployment.
- The only way to pass the admin check before the security spec ships is a test-only credential that exists solely inside the automated test harness. It is never available in a built or deployed API.

### 4.5 Response envelopes

Single resource:

```json
{ "data": { ... } }
```

List:

```json
{ "data": [ ... ], "pagination": { "page": 1, "limit": 20, "total_items": 342, "total_pages": 18 } }
```

Error:

```json
{ "error": { "code": "SPECIES_NOT_FOUND", "message": "No species found with id 'trex01'." } }
```

Validation errors (`422`) also include a `details` array with one entry per problem. Every problem is reported, not only the first. A domain rule that involves several fields (for example an overlap) is reported on the field the admin most likely needs to change:

```json
{
  "error": {
    "code": "VALIDATION_FAILED",
    "message": "The request body has 2 invalid fields.",
    "details": [
      { "field": "end_mya", "message": "end_mya (201.4) must be less than start_mya (145.0)." },
      { "field": "id", "message": "id must be 2–64 lowercase letters, digits, or hyphens." }
    ]
  }
}
```

Error messages say what went wrong and how to fix it. They never include stack traces, queries, or internal details.

### 4.6 Pagination

- Every list endpoint is paginated with `?page=<n>&limit=<n>`.
- Defaults: `page=1`, `limit=20`. Allowed range: `1 ≤ page ≤ 1,000,000`, `1 ≤ limit ≤ 100`.
- A value that is not an integer, is out of range, or is empty (`?page=`) returns `400`.
- A `page` past the last page returns `200` with empty `data` and correct `pagination` totals.
- `total_pages` is `0` when `total_items` is `0`.

### 4.7 Sorting

- List endpoints accept `?sort=<field>` for ascending order or `?sort=-<field>` for descending order.
- Only the fields listed for each resource are sortable. Any other field returns `400` and the message lists the allowed fields.
- Text fields sort case-insensitively.
- Ties are broken by `id` ascending so ordering is stable across pages.

### 4.8 Query parameters and filtering

- Filters are query parameters. Multiple filters combine with **AND**.
- An invalid filter value returns `400`, for example `diet=banana`, an empty value (`diet=`), or an `id` filter that is not a valid slug.
- A well-formed filter that references a non-existent resource returns `200` with empty `data`, for example `era=unknown-era`.
- Repeating a known query parameter (`?page=1&page=2`) returns `400`.
- Unknown query parameters are ignored.

### 4.9 Caching

Data changes rarely, so reads may be served from HTTP caches, and clients can always ask for fresh data.

- Every successful `GET` response includes an `ETag` and `Cache-Control: public, max-age=300`.
- After a write, clients and caches may keep showing the previous representation until `max-age` expires. This staleness is accepted.
- A client can force fresh data at any time by revalidating (for example `Cache-Control: no-cache` on the request, or a browser hard reload). The API always answers from current data and never from a server-side cache.
- A `GET` with an `If-None-Match` header matching the current `ETag` returns `304 Not Modified` with no body. A `304` still carries the `ETag`, `Cache-Control`, and CORS headers.
- A resource's `ETag` changes when its representation changes, including changes to embedded related data. A list's `ETag` changes when any item, the order, or the totals change.

### 4.10 Write semantics

| Operation | Success | Body |
|---|---|---|
| `POST` (create) | `201 Created` + `Location` header pointing to the new resource's canonical path | `{ "data": <created resource> }` |
| `PATCH` (partial update) | `200 OK` | `{ "data": <updated resource> }` |
| `DELETE` | `204 No Content` | none |

- Write bodies must be sent as `application/json`. Any other `Content-Type` returns `415`. Bodies larger than 64 KB return `413`.
- `PATCH` changes only the fields sent. Omitted fields are unchanged.
- `PATCH` validation applies to the **resulting** resource (stored values merged with the sent fields), so all rules hold after every update. For example, sending only `end_mya` is checked against the stored `start_mya`.
- Sending `null` clears an optional field. Sending `null` for a required field returns `422`. Sending `null` for an optional list clears it to an empty list.
- For list-valued fields (for example `period_ids`) and object-valued fields (`size`), a `PATCH` value **replaces** the whole value. There is no deep merge.
- A `PATCH` with an empty body `{}` returns `422`.
- Unknown fields in a write body return `422` so typos are caught. Response-only fields (for example `era`, `taxonomy`) count as unknown.
- A write that references another resource that does not exist (for example `genus_id: "nope"`) returns `422` with a field detail. It does not return `404`.
- **Delete protection:** deleting a resource that other resources still depend on returns `409`. The message states how many dependents block the delete and names up to 5 of them by id. Admins must remove or reassign dependents first. There is no cascading delete.
- **Check order.** When a request has several problems, the first applicable response wins:
  1. `404` for an unknown route, `405` for an unsupported method
  2. `401` / `403` for the admin check
  3. `413` / `415` / `400` for body size, content type, and malformed JSON
  4. `404` for an unknown resource in the path (including a parent in a nested create)
  5. `422` for validation and domain rules (all problems reported together)
  6. `409` for duplicate `id` or name, and for delete protection

### 4.11 Status codes

| Code | When |
|---|---|
| `200` | Successful read or update |
| `201` | Successful create |
| `204` | Successful delete |
| `304` | `If-None-Match` matches current `ETag` |
| `400` | Malformed JSON, or an invalid query parameter (type, range, empty, repeated, unknown sort field, invalid filter value) |
| `401` | Write without valid credentials |
| `403` | Write by a non-admin |
| `404` | Resource or route not found |
| `405` | Method not supported on the route |
| `409` | Duplicate `id` or unique name; delete blocked by dependents |
| `413` | Request body larger than 64 KB |
| `415` | Write body is not `application/json` |
| `422` | Well-formed body that breaks validation or domain rules |
| `500` | Unexpected error (generic message only) |

### 4.12 Error codes

| Code | Status |
|---|---|
| `INVALID_JSON` | 400 |
| `INVALID_QUERY_PARAMETER` | 400 |
| `UNAUTHORIZED` | 401 |
| `FORBIDDEN` | 403 |
| `<RESOURCE>_NOT_FOUND` (for example `ERA_NOT_FOUND`, `GENUS_NOT_FOUND`) | 404 |
| `ROUTE_NOT_FOUND` | 404 |
| `METHOD_NOT_ALLOWED` | 405 |
| `<RESOURCE>_ALREADY_EXISTS` | 409 |
| `<RESOURCE>_HAS_DEPENDENTS` | 409 |
| `PAYLOAD_TOO_LARGE` | 413 |
| `UNSUPPORTED_MEDIA_TYPE` | 415 |
| `VALIDATION_FAILED` | 422 |
| `INTERNAL_ERROR` | 500 |

### 4.13 Common field rules

- **Text fields** have surrounding whitespace trimmed before validation and storage. Lengths are counted in Unicode characters after trimming. A value that is empty after trimming counts as missing.
- `name`: required, 1–64 characters, unique within its resource type (case-insensitive), except where a resource says otherwise. A duplicate returns `409`. Renaming a resource to a different casing of its own name is allowed.
- **Decimals** are sent and returned as JSON numbers. A value with more decimal places than a field allows returns `422`. It is never silently rounded.
- **Lists of ids** reject duplicates with `422`. Each list field has a maximum length, stated per resource.
- A **summary** of a related resource is `{ "id": "...", "name": "..." }`. Continent summaries also include `type`.

## 5. Resources

### 5.1 Eras

**Fields**

| Field | Type | Rules |
|---|---|---|
| `id` | slug | required on create; immutable |
| `name` | string | required; unique |
| `start_mya` | decimal | required; `0 ≤ start_mya ≤ 4600`; at most 3 decimal places; millions of years ago |
| `end_mya` | decimal | required; same bounds and precision; `start_mya > end_mya` |

**Domain rules**

- Eras must not overlap. Touching boundaries are allowed, for example Mesozoic 251.902–66.0 and Cenozoic 66.0–0. Gaps between eras are allowed.
- Changing an era's range must still contain all of its periods. Otherwise `422`.

**Endpoints**

| Method | Path | Notes |
|---|---|---|
| GET | `/eras` | Sort: `start_mya` (default `-start_mya`, oldest first), `name` |
| GET | `/eras/{era_id}` | |
| POST | `/eras` | |
| PATCH | `/eras/{era_id}` | |
| DELETE | `/eras/{era_id}` | `409` if the era has periods |

### 5.2 Periods

**Fields**

| Field | Type | Rules |
|---|---|---|
| `id` | slug | required on create; immutable |
| `name` | string | required; unique |
| `start_mya`, `end_mya` | decimal | same rules as eras |
| `era_id` | slug | write only; set by the create path; may be changed by `PATCH`. The period belongs to exactly one era. |

**Response** includes `era` as a summary.

**Domain rules**

- A period's range lies within its era's range. Boundaries may coincide.
- Periods of the same era must not overlap. Touching boundaries are allowed.
- `PATCH` may change `era_id` to move a period, together with its range if needed. The rules above are checked against the resulting period.

**Endpoints**

| Method | Path | Notes |
|---|---|---|
| GET | `/eras/{era_id}/periods` | Periods of one era. `404` if the era does not exist. Sort: `start_mya` (default `-start_mya`), `name` |
| POST | `/eras/{era_id}/periods` | Creates the period in that era. `era_id` comes from the path, and sending `era_id` in the body returns `422`. `Location` is `/api/v1/periods/{period_id}`. |
| GET | `/periods` | All periods. Filter: `era`. Same sort options. |
| GET | `/periods/{period_id}` | |
| PATCH | `/periods/{period_id}` | |
| DELETE | `/periods/{period_id}` | `409` if any species is linked to the period |

### 5.3 Taxonomy ranks

Seven ranks form a strict hierarchy. Each rank except `domain` has exactly one parent from the rank directly above it.

| Rank | Plural path segment | Parent |
|---|---|---|
| Domain | `domains` | none |
| Kingdom | `kingdoms` | domain |
| Phylum | `phyla` | kingdom |
| Class | `classes` | phylum |
| Order | `orders` | class |
| Family | `families` | order |
| Genus | `genera` | family |

**Fields (every rank)**

| Field | Type | Rules |
|---|---|---|
| `id` | slug | required on create; immutable |
| `name` | string | required; unique within the rank |
| `<parent>_id` | slug | write only; every rank except domain, for example `domain_id` on a kingdom. Set by the create path; may be changed by `PATCH`. |

**Response** includes the parent as a summary under the parent rank's name, for example a kingdom has `"domain": { "id": "eukaryota", "name": "Eukaryota" }`.

**Endpoints**

All paths are under `/api/v1/taxonomy`. `{rank}` is the plural segment and `{parent_rank}` is the plural segment of its **direct** parent. Any other rank pairing (for example `/domains/eukaryota/genera`) returns `404 ROUTE_NOT_FOUND`.

| Method | Path | Notes |
|---|---|---|
| GET | `/{rank}` | All items of the rank. Filter: `<parent>` (for example `/kingdoms?domain=eukaryota`). Sort: `name` (default). |
| GET | `/{parent_rank}/{parent_id}/{rank}` | Children of one parent, for example `/domains/eukaryota/kingdoms`. `404` if the parent does not exist. |
| GET | `/{rank}/{id}` | |
| POST | `/domains` | Creates a domain, which has no parent. |
| POST | `/{parent_rank}/{parent_id}/{rank}` | Creates a child under that parent. Sending the parent id in the body returns `422`. `Location` is `/api/v1/taxonomy/{rank}/{id}`. |
| PATCH | `/{rank}/{id}` | May change `<parent>_id` to reparent. |
| DELETE | `/{rank}/{id}` | `409` if the item has children. Also `409` if a genus has species. |

### 5.4 Continents

**Fields**

| Field | Type | Rules |
|---|---|---|
| `id` | slug | required on create; immutable |
| `name` | string | required; unique |
| `type` | enum | required; `prehistoric` or `modern` |

**Domain rules**

- Only `modern` continents can contain countries.
- Changing `type` from `modern` to `prehistoric` while countries are linked returns `422`.

**Endpoints**

| Method | Path | Notes |
|---|---|---|
| GET | `/continents` | Filter: `type`. Sort: `name` (default). |
| GET | `/continents/{continent_id}` | |
| GET | `/continents/{continent_id}/countries` | `404` if the continent does not exist. Returns an empty list for a prehistoric continent. Sort: `name`. |
| POST | `/continents` | |
| PATCH | `/continents/{continent_id}` | |
| DELETE | `/continents/{continent_id}` | `409` if linked to any species or country |

### 5.5 Countries

**Fields**

| Field | Type | Rules |
|---|---|---|
| `id` | slug | required on create; immutable |
| `name` | string | required; unique |
| `continent_ids` | list of slugs | write only; required; 1–10 items; each must be a `modern` continent; no duplicates. A country may belong to several continents, for example Russia or Egypt. |

**Response** includes `continents` as a list of summaries, sorted by `name`.

**Endpoints**

| Method | Path | Notes |
|---|---|---|
| GET | `/countries` | Filter: `continent`. Sort: `name` (default). |
| GET | `/countries/{country_id}` | |
| POST | `/countries` | |
| PATCH | `/countries/{country_id}` | |
| DELETE | `/countries/{country_id}` | `409` if any species is linked to the country. Otherwise allowed, and removes the country's continent links. |

### 5.6 Species

**Fields (write)**

| Field | Type | Rules |
|---|---|---|
| `id` | slug | required on create; immutable |
| `name` | string | required; common name; 1–64 characters; **not unique** (congeners may share a common name) |
| `scientific_name` | string | required; 1–128 characters; unique (case-insensitive). No format rule is enforced. |
| `diet` | enum | required; one of `carnivore`, `herbivore`, `omnivore`, `piscivore`, `insectivore` |
| `description` | string | required; 1–1000 characters |
| `genus_id` | slug | required; must exist |
| `period_ids` | list of slugs | required; 1–20 items; must exist; no duplicates |
| `continent_ids` | list of slugs | optional (defaults to empty); 0–20 items; must exist; no duplicates; may mix `prehistoric` and `modern` |
| `country_ids` | list of slugs | optional (defaults to empty); 0–50 items; must exist; no duplicates. Countries where fossils were found. Independent of `continent_ids`: no consistency rule links the two lists. |
| `size` | object or `null` | optional; see below |
| `discovery_year` | integer or `null` | optional; between 1600 and the current year (UTC) |
| `image_url` | string or `null` | optional; absolute `http` or `https` URL; at most 2048 characters |

**`size` object.** It must contain at least one sub-object. To remove all size data, send `"size": null`. Each present sub-object needs both bounds as decimals `> 0` with at most 6 decimal places and `min ≤ max`. A sub-object sent as `null` is the same as leaving it out. Unknown sub-object keys return `422`.

```json
{
  "length_m":  { "min": 11.0, "max": 12.3 },
  "height_m":  { "min": 3.6,  "max": 4.0 },
  "weight_kg": { "min": 5000, "max": 8000 }
}
```

**Response shape.** List items and detail use the same shape so a single fetch renders a full card:

```json
{
  "id": "tyrannosaurus-rex",
  "name": "Tyrannosaurus",
  "scientific_name": "Tyrannosaurus rex",
  "diet": "carnivore",
  "description": "...",
  "discovery_year": 1905,
  "image_url": "https://example.org/trex.png",
  "size": { "length_m": { "min": 11.0, "max": 12.3 }, "height_m": null, "weight_kg": { "min": 5000, "max": 8000 } },
  "taxonomy": {
    "domain":  { "id": "eukaryota", "name": "Eukaryota" },
    "kingdom": { "id": "animalia", "name": "Animalia" },
    "phylum":  { "id": "chordata", "name": "Chordata" },
    "class":   { "id": "reptilia", "name": "Reptilia" },
    "order":   { "id": "saurischia", "name": "Saurischia" },
    "family":  { "id": "tyrannosauridae", "name": "Tyrannosauridae" },
    "genus":   { "id": "tyrannosaurus", "name": "Tyrannosaurus" }
  },
  "periods": [
    { "id": "cretaceous", "name": "Cretaceous", "era": { "id": "mesozoic", "name": "Mesozoic" } }
  ],
  "continents": [
    { "id": "laramidia", "name": "Laramidia", "type": "prehistoric" },
    { "id": "north-america", "name": "North America", "type": "modern" }
  ],
  "countries": [
    { "id": "canada", "name": "Canada" },
    { "id": "usa", "name": "United States" }
  ]
}
```

- `taxonomy` is always the full lineage from domain to genus. It is derived from `genus_id`.
- `size` is `null` when no size is set. Absent size sub-objects are `null`.
- `periods` is sorted oldest first. `continents` is sorted by `type` (prehistoric first), then `name`. `countries` is sorted by `name`.

**Filters (`GET /species`)**

| Param | Meaning |
|---|---|
| `diet` | Exact diet value |
| `era` | Species linked to any period of that era |
| `period` | Species linked to that period |
| `domain`, `kingdom`, `phylum`, `class`, `order`, `family`, `genus` | Species whose lineage includes that rank item |
| `continent` | Species linked to that continent (prehistoric or modern) |
| `country` | Species linked directly to that country |
| `q` | Case-insensitive substring match on `name` or `scientific_name`; 3–64 characters after trimming. Every character matches literally (`%` and `_` are not wildcards). |

Sort: `name` (default), `scientific_name`, `discovery_year`. Species without `discovery_year` sort last in both directions.

**Endpoints**

| Method | Path | Notes |
|---|---|---|
| GET | `/species` | Filters and sort as above |
| GET | `/species/{species_id}` | |
| POST | `/species` | |
| PATCH | `/species/{species_id}` | `period_ids` must still have at least 1 item |
| DELETE | `/species/{species_id}` | Always allowed. Removes the species' size and links. |

## 6. Acceptance Criteria

Each criterion is testable. `R` means any resource in §5.

### 6.1 Global

1. `GET` on any list endpoint without query params returns `200`, `page=1`, `limit=20`, and at most 20 items.
2. `limit=101`, `limit=0`, `page=0`, `page=1000001`, `page=abc`, `page=`, or `page=1&page=2` returns `400 INVALID_QUERY_PARAMETER`.
3. `page` past the last page returns `200`, empty `data`, and correct `total_items` and `total_pages`.
4. `sort=unknown` returns `400` and the message lists the allowed sort fields.
5. Repeating a list request with the same params returns the same order (stable sort with `id` tie-break). Text sorts ignore case.
6. Every `GET 200` has an `ETag` and `Cache-Control: public, max-age=300`. Repeating with `If-None-Match: <etag>` returns `304` with no body and with the `ETag` header.
7. After a successful write to R, a `GET` on R that bypasses caches returns the new data and a different `ETag`. A conditional `GET` with the old `ETag` returns `200`, not `304`.
8. A `GET` response includes `Access-Control-Allow-Origin: *` and `Access-Control-Expose-Headers` containing `ETag`. An `OPTIONS` preflight on any route succeeds.
9. Before the security spec ships, every write without the test-only credential returns `401 UNAUTHORIZED`, including writes to unknown ids. After it ships: a write without credentials returns `401 UNAUTHORIZED`, and a write with non-admin credentials returns `403 FORBIDDEN`.
10. Malformed JSON returns `400 INVALID_JSON`. A non-JSON `Content-Type` returns `415`. A body over 64 KB returns `413`.
11. An unknown body field, a response-only field, an `id` in a `PATCH` body, or an empty `PATCH` body returns `422 VALIDATION_FAILED`.
12. A body with several invalid fields returns one `details` entry per problem.
13. An invalid slug `id` on create (including one over 64 characters) returns `422`. A duplicate `id` or duplicate name (any casing) returns `409 <R>_ALREADY_EXISTS`. Renaming R to a different casing of its own name succeeds.
14. `GET`, `PATCH`, or `DELETE` on a non-existent `id` returns `404 <R>_NOT_FOUND` with the id in the message.
15. An unknown route returns `404 ROUTE_NOT_FOUND`. An unsupported method returns `405`.
16. A create returns `201`, a `Location` header, and the body equals a subsequent `GET` of that location.
17. A delete returns `204`, and a subsequent `GET` returns `404`.
18. No error body contains stack traces, SQL, or internal type names.
19. A `name` of only whitespace returns `422`. A `name` with surrounding whitespace is stored trimmed.
20. A `409 <R>_HAS_DEPENDENTS` message states the dependent count and names at most 5 dependent ids.
21. A body that is both invalid (`422`) and has a duplicate `id` returns `422`.

### 6.2 Eras and periods

1. An era or period with `start_mya ≤ end_mya` returns `422`.
2. Creating an era that overlaps an existing era returns `422`. An era that only touches a boundary succeeds.
3. A period outside its era's range returns `422`. A period overlapping a sibling period returns `422`.
4. Shrinking an era so that one of its periods falls outside it returns `422`.
5. Deleting an era with periods returns `409 ERA_HAS_DEPENDENTS`. Deleting a period linked to a species returns `409 PERIOD_HAS_DEPENDENTS`.
6. `GET /eras` is ordered oldest first by default.
7. `GET /eras/{unknown}/periods` returns `404 ERA_NOT_FOUND`.
8. `PATCH /periods/{id}` with a new `era_id` moves the period when the rules hold. Moving it with a new range in the same request is checked against the new era and range.
9. `PATCH` with only `end_mya` set above the stored `start_mya` returns `422`.
10. `start_mya` above 4600, below 0, or with more than 3 decimal places returns `422`.
11. `POST /eras/{id}/periods` returns a `Location` of `/api/v1/periods/{period_id}`.

### 6.3 Taxonomy

1. Creating a child through `POST /{parent_rank}/{unknown}/{rank}` returns `404`.
2. `GET /taxonomy/domains/{id}/kingdoms` returns only kingdoms of that domain.
3. Deleting a rank item that has children returns `409`. Deleting a genus with species returns `409 GENUS_HAS_DEPENDENTS`.
4. Reparenting via `PATCH` with a non-existent parent returns `422`.
5. Each rank response includes its parent summary. A domain has no parent field.
6. A nested path with a non-direct parent (for example `/taxonomy/domains/{id}/genera`) returns `404 ROUTE_NOT_FOUND`.

### 6.4 Geography

1. A country with an empty `continent_ids`, or linked to a prehistoric continent, returns `422`.
2. `GET /continents/{id}/countries` lists only that continent's countries.
3. Changing a continent with countries from `modern` to `prehistoric` returns `422`.
4. Deleting a continent linked to countries or species returns `409`.
5. Deleting a country with no linked species succeeds, and the country disappears from its continents' country lists.
6. Deleting a country linked to a species returns `409 COUNTRY_HAS_DEPENDENTS`.

### 6.5 Species

1. A create missing `genus_id` or `period_ids`, or with an empty `period_ids`, returns `422`.
2. A create referencing a non-existent genus, period, continent, or country returns `422` with the offending field in `details`.
3. `diet` outside the enum returns `422` on write and `400` as a filter.
4. A size with `min > max`, a bound `≤ 0`, more than 6 decimal places, or an empty `size` object `{}` returns `422`.
5. `discovery_year` in the future or before 1600 returns `422`. A non-http(s) `image_url` returns `422`.
6. The response contains the full taxonomy lineage, periods with their era, continents with their type, and countries, in the orders defined in §5.6.
7. Each filter in §5.6 returns only matching species. Combined filters apply AND.
8. `q=REX` matches "Tyrannosaurus rex" through its scientific name, case-insensitively. `q=re` returns `400`. `q=%%_` matches only names containing `%%_` literally.
9. `country=usa` returns only species whose `country_ids` include `usa`, regardless of their continents.
10. `PATCH` with `period_ids` replaces the whole list. `PATCH` with `"size": null` clears the size. `PATCH` with `size` containing only `length_m` removes any stored `height_m` and `weight_kg`.
11. Reparenting a genus changes the `taxonomy` lineage and `ETag` of its species.
12. Deleting a species returns `204` and never `409`.
13. Two species with the same `name` and different `scientific_name` can both be created. A duplicate `scientific_name` (any casing) returns `409 SPECIES_ALREADY_EXISTS`.
14. `period_ids` with more than 20 items, `continent_ids` with more than 20, or `country_ids` with more than 50 returns `422`.

## 7. Non-Functional Requirements

- Read endpoints meet the Constitution III targets: p95 below 50 ms at the server, and no unbounded or N+1 reads. This includes species lists with embedded relations and the `q` substring search.
- Every endpoint in this spec is documented in the OpenAPI description, including examples and error responses.
- Behavior is covered by unit tests (validation and domain rules) and integration tests (HTTP contract against the real PostgreSQL provided by Spec 001), written before implementation.
- Domain rules (no overlaps, containment, references, uniqueness) also hold at the database level, as delivered by Spec 001 (Constitution V). No sequence of writes can leave the data inconsistent.

## 8. Constitution Check

| Principle | Compliance |
|---|---|
| I. Easy to Fetch | ✅ Public `GET`, open CORS with exposed `ETag`, uniform envelopes, `snake_case`, embedded summaries so one fetch renders a card, human-readable errors with field details. |
| II. Simple CRUD, Restricted Writes | ✅ Plain CRUD with no extra features. Writes are admin-only and deny-by-default: until the security spec ships, every write is rejected with `401` in every deployment, so writes are never exposed without authentication and role checks (§4.4). |
| III. High Performance | ✅ Pagination is mandatory with a max of 100. `ETag`, `Cache-Control`, and `304` are required. Embedded lists are capped. Indexes are delivered by Spec 001, and N+1 avoidance is carried into the plan (§7). Cache staleness of up to 5 minutes after writes is accepted because writes are rare. |
| IV. Test-First | ✅ §6 gives testable acceptance criteria for tests written first. Integration tests run against the real PostgreSQL delivered by Spec 001, which is a prerequisite. |
| V. Data Integrity & Accuracy | ✅ Strict taxonomy hierarchy, a period belongs to one era, `start > end`, no era or period overlaps, `min ≤ max`, bounded time values, delete protection instead of cascades. The rules are enforced at the database level too (§7). |
| VI. Simplicity | ✅ Only `PATCH` (no `PUT`), no deep merge, no `?include=` options, no cascades, single-value filters, HTTP caching only (no server-side cache, process stays stateless). |

**Deviations:** none.

## 9. Out of Scope / Dependencies

- **Spec 001 — Database Foundation (prerequisite):** PostgreSQL for development and tests, migrations, the schema for the resources in §5, database-level integrity constraints, and indexes. It replaces the `src/db/shemas_definition.sql` draft.
- **Security spec:** login, JWT issuance and verification, admin role, password hashing, rate limiting (`429`). It replaces the deny-all admin check in §4.4.
- Seed data and bulk import.
- Image hosting (only `image_url` is stored).
- Multi-value filters (`diet=a,b`), full-text search, and `?fields=` selection.
- Server-side caching and cache purging.
- API versioning beyond `v1`.
