//! Species (spec §5.6).

pub mod card;
pub mod input;
pub mod list;

use actix_web::{HttpRequest, HttpResponse, web};
use sqlx::{PgConnection, PgPool};

use self::card::{SpeciesRow, load_cards};
use self::input::{Refs, check_refs, read_create, read_patch};
use self::list::{DEFAULT_SORT, SORTS, SpeciesFilter, SpeciesQuery};
use super::auth::AdminGate;
use super::db_error::{Ctx, Op, map_write};
use super::error::{ApiError, Resource};
use super::http::{Many, One, cached_json, created, updated};
use super::input::{Obj, is_slug};
use super::params::{Pagination, Params};
use super::{begin_write, delete_resource, read_tx, write_body};

/// `diet` values (spec §5.6).
pub const DIETS: &[&str] = &[
    "carnivore",
    "herbivore",
    "omnivore",
    "piscivore",
    "insectivore",
];

/// Query parameters of `GET /species`.
const KNOWN: &[&str] = &[
    "page",
    "limit",
    "sort",
    "diet",
    "era",
    "period",
    "domain",
    "kingdom",
    "phylum",
    "class",
    "order",
    "family",
    "genus",
    "continent",
    "country",
    "q",
];

fixed_query! {
    /// Q-S13: one species by id.
    pub const SPECIES_BY_ID_SQL = "SELECT s.id, s.genus_id, s.name, s.scientific_name, s.diet, s.description, \
            s.discovery_year, s.image_url, s.length_min_m, s.length_max_m, s.height_min_m, \
            s.height_max_m, s.weight_min_kg, s.weight_max_kg \
        FROM species s WHERE s.id = $1";
    pub(crate) fn species_by_id(id: &str) -> fetch_optional SpeciesRow;
}

/// Parses the query string of `GET /species`.
fn parse_list(req: &HttpRequest) -> Result<SpeciesQuery, ApiError> {
    let p = Params::parse(req.query_string(), KNOWN)?;
    Ok(SpeciesQuery {
        page: p.page()?,
        sort: p.sort(SORTS, DEFAULT_SORT)?,
        filter: SpeciesFilter {
            diet: p.one_of("diet", DIETS)?,
            era: p.slug("era")?,
            period: p.slug("period")?,
            domain: p.slug("domain")?,
            kingdom: p.slug("kingdom")?,
            phylum: p.slug("phylum")?,
            class: p.slug("class")?,
            order: p.slug("order")?,
            family: p.slug("family")?,
            genus: p.slug("genus")?,
            continent: p.slug("continent")?,
            country: p.slug("country")?,
            q: p.q()?,
        },
    })
}

/// `GET /species`.
pub async fn list(req: HttpRequest, pool: web::Data<PgPool>) -> Result<HttpResponse, ApiError> {
    let q = parse_list(&req)?;
    let mut tx = read_tx(&pool).await?;
    let (total, rows) = list::fetch(&mut tx, &q).await?;
    let data = load_cards(&mut tx, rows).await?;
    tx.commit().await?;
    Ok(cached_json(
        &req,
        &Many {
            data,
            pagination: Pagination::new(q.page, total),
        },
    ))
}

/// Loads one card, or `None` when the species does not exist.
pub(crate) async fn load_one(
    conn: &mut sqlx::PgConnection,
    id: &str,
) -> sqlx::Result<Option<card::SpeciesCard>> {
    let Some(row) = species_by_id(conn, id).await? else {
        return Ok(None);
    };
    Ok(load_cards(conn, vec![row]).await?.pop())
}

/// `GET /species/{species_id}`.
pub async fn detail(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Species, &id));
    }
    let mut tx = read_tx(&pool).await?;
    let card = load_one(&mut tx, &id).await?;
    tx.commit().await?;
    match card {
        Some(data) => Ok(cached_json(&req, &One { data })),
        None => Err(ApiError::not_found(Resource::Species, &id)),
    }
}

// ---------------------------------------------------------------- write handlers

/// Q-W12: replaces the period, continent, and country links that were sent.
async fn replace_links(
    conn: &mut PgConnection,
    id: &str,
    periods: Option<&[String]>,
    continents: Option<&[String]>,
    countries: Option<&[String]>,
) -> sqlx::Result<()> {
    if let Some(ids) = periods {
        sqlx::query!(
            "DELETE FROM species_periods WHERE species_id = $1 AND period_id <> ALL($2::text[])",
            id,
            ids
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query!(
            "INSERT INTO species_periods (species_id, period_id) \
             SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
            id,
            ids
        )
        .execute(&mut *conn)
        .await?;
    }
    if let Some(ids) = continents {
        sqlx::query!(
            "DELETE FROM species_continents WHERE species_id = $1 AND continent_id <> ALL($2::text[])",
            id,
            ids
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query!(
            "INSERT INTO species_continents (species_id, continent_id) \
             SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
            id,
            ids
        )
        .execute(&mut *conn)
        .await?;
    }
    if let Some(ids) = countries {
        sqlx::query!(
            "DELETE FROM species_countries WHERE species_id = $1 AND country_id <> ALL($2::text[])",
            id,
            ids
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query!(
            "INSERT INTO species_countries (species_id, country_id) \
             SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
            id,
            ids
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// `POST /species`.
pub async fn create(
    req: HttpRequest,
    payload: web::Payload,
    pool: web::Data<PgPool>,
    gate: web::Data<dyn AdminGate>,
) -> Result<HttpResponse, ApiError> {
    let body = write_body(&req, payload, gate.get_ref()).await?;
    let mut tx = begin_write(&pool).await?;
    let mut o = Obj::new(body)?;
    let draft = read_create(&mut o);
    check_refs(
        &mut tx,
        &mut o,
        Refs {
            genus_id: draft.genus_id.as_deref(),
            period_ids: draft.period_ids.as_deref(),
            continent_ids: draft.continent_ids.as_deref(),
            country_ids: draft.country_ids.as_deref(),
            discovery_year: draft.discovery_year.flatten(),
        },
    )
    .await?;
    o.finish()?;
    let s = draft.complete().ok_or_else(ApiError::internal)?;
    let ctx = Ctx::new(Resource::Species, &s.id, Op::Insert).name(Some(&s.scientific_name));
    let [lmin, lmax, hmin, hmax, wmin, wmax] = s.size.columns();
    sqlx::query!(
        "INSERT INTO species (id, genus_id, name, scientific_name, diet, description, \
            discovery_year, image_url, length_min_m, length_max_m, height_min_m, height_max_m, \
            weight_min_kg, weight_max_kg) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        s.id,
        s.genus_id,
        s.name,
        s.scientific_name,
        s.diet,
        s.description,
        s.discovery_year,
        s.image_url,
        lmin,
        lmax,
        hmin,
        hmax,
        wmin,
        wmax
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    replace_links(
        &mut tx,
        &s.id,
        Some(&s.period_ids),
        Some(&s.continent_ids),
        Some(&s.country_ids),
    )
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    let card = load_one(&mut tx, &s.id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await.map_err(|e| map_write(&e, &ctx))?;
    Ok(created(
        format!("/api/v1/species/{}", s.id),
        &One { data: card },
    ))
}

/// `PATCH /species/{species_id}`: lists and `size` replace the stored value.
pub async fn update(
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Payload,
    pool: web::Data<PgPool>,
    gate: web::Data<dyn AdminGate>,
) -> Result<HttpResponse, ApiError> {
    let body = write_body(&req, payload, gate.get_ref()).await?;
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Species, &id));
    }
    let mut tx = begin_write(&pool).await?;
    let locked = sqlx::query_scalar!("SELECT id FROM species WHERE id = $1 FOR NO KEY UPDATE", id)
        .fetch_optional(&mut *tx)
        .await?;
    if locked.is_none() {
        return Err(ApiError::not_found(Resource::Species, &id));
    }
    let mut o = Obj::new(body)?;
    let p = read_patch(&mut o);
    check_refs(
        &mut tx,
        &mut o,
        Refs {
            genus_id: p.genus_id.as_deref(),
            period_ids: p.period_ids.as_deref(),
            continent_ids: p.continent_ids.as_deref(),
            country_ids: p.country_ids.as_deref(),
            discovery_year: p.discovery_year.flatten(),
        },
    )
    .await?;
    o.finish()?;
    let ctx = Ctx::new(Resource::Species, &id, Op::Update).name(p.scientific_name.as_deref());
    let size = p.size.as_ref().map(|s| s.columns()).unwrap_or_default();
    let [lmin, lmax, hmin, hmax, wmin, wmax] = size;
    sqlx::query!(
        "UPDATE species SET \
            name = COALESCE($2, name), \
            scientific_name = COALESCE($3, scientific_name), \
            diet = COALESCE($4, diet), \
            description = COALESCE($5, description), \
            genus_id = COALESCE($6, genus_id), \
            discovery_year = CASE WHEN $7 THEN $8::int ELSE discovery_year END, \
            image_url = CASE WHEN $9 THEN $10::text ELSE image_url END, \
            length_min_m = CASE WHEN $11 THEN $12::numeric ELSE length_min_m END, \
            length_max_m = CASE WHEN $11 THEN $13::numeric ELSE length_max_m END, \
            height_min_m = CASE WHEN $11 THEN $14::numeric ELSE height_min_m END, \
            height_max_m = CASE WHEN $11 THEN $15::numeric ELSE height_max_m END, \
            weight_min_kg = CASE WHEN $11 THEN $16::numeric ELSE weight_min_kg END, \
            weight_max_kg = CASE WHEN $11 THEN $17::numeric ELSE weight_max_kg END \
         WHERE id = $1",
        id,
        p.name,
        p.scientific_name,
        p.diet,
        p.description,
        p.genus_id,
        p.discovery_year.is_some(),
        p.discovery_year.flatten(),
        p.image_url.is_some(),
        p.image_url.clone().flatten(),
        p.size.is_some(),
        lmin,
        lmax,
        hmin,
        hmax,
        wmin,
        wmax
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    replace_links(
        &mut tx,
        &id,
        p.period_ids.as_deref(),
        p.continent_ids.as_deref(),
        p.country_ids.as_deref(),
    )
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    let card = load_one(&mut tx, &id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await.map_err(|e| map_write(&e, &ctx))?;
    Ok(updated(&One { data: card }))
}

/// `DELETE /species/{species_id}`: always allowed; links and size go with it.
pub async fn delete(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
    gate: web::Data<dyn AdminGate>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    delete_resource(
        &req,
        gate.get_ref(),
        &pool,
        Resource::Species,
        &id,
        async |c: &mut PgConnection| {
            Ok(
                sqlx::query!("SELECT 1 AS one FROM species WHERE id = $1 FOR UPDATE", id)
                    .fetch_optional(c)
                    .await?
                    .is_some(),
            )
        },
        async |c: &mut PgConnection| {
            sqlx::query!("DELETE FROM species WHERE id = $1", id)
                .execute(c)
                .await
                .map(|_| ())
        },
    )
    .await
}
