//! Continents and countries (spec §5.4, §5.5).

use std::collections::HashMap;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Serialize;
use sqlx::{PgConnection, PgPool, Postgres, QueryBuilder};

use super::error::{ApiError, Resource};
use super::http::{Many, One, cached_json};
use super::input::is_slug;
use super::params::{ListQuery, Pagination, Params, Sort};
use super::species::card::ContinentSummary;
use super::{paged, read_tx};

// ---------------------------------------------------------------- representations

/// A continent: `id`, `name`, `type` (spec §5.4).
pub type Continent = ContinentSummary;

/// A country with its continents sorted by name (spec §5.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Country {
    pub id: String,
    pub name: String,
    pub continents: Vec<ContinentSummary>,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ContinentRow {
    id: String,
    name: String,
    kind: String,
}

impl From<ContinentRow> for Continent {
    fn from(r: ContinentRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            kind: r.kind,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct CountryRow {
    id: String,
    name: String,
}

#[derive(Debug)]
struct CountryContinentRow {
    country_id: String,
    id: String,
    name: String,
    kind: String,
}

fixed_query! {
    /// One continent by id (PK).
    pub const CONTINENT_BY_ID_SQL = "SELECT c.id, c.name, c.type AS kind FROM continents c WHERE c.id = $1";
    pub(crate) fn continent_by_id(id: &str) -> fetch_optional ContinentRow;
}

fixed_query! {
    /// Q-K2: one country by id (PK).
    pub const COUNTRY_BY_ID_SQL = "SELECT k.id, k.name FROM countries k WHERE k.id = $1";
    pub(crate) fn country_by_id(id: &str) -> fetch_optional CountryRow;
}

fixed_query! {
    /// Q-K2: continents of a page of countries, by name, in one statement.
    pub const COUNTRY_CONTINENTS_SQL = "SELECT cc.country_id, c.id, c.name, c.type AS kind \
        FROM country_continents cc JOIN continents c ON c.id = cc.continent_id \
        WHERE cc.country_id = ANY($1::text[]) \
        ORDER BY lower(c.name) COLLATE paleo_name_sort, c.id";
    fn country_continent_rows(ids: &[String]) -> fetch_all CountryContinentRow;
}

/// Builds countries (in order) with one embedding statement.
pub(crate) async fn load_countries(
    conn: &mut PgConnection,
    rows: Vec<CountryRow>,
) -> sqlx::Result<Vec<Country>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut links: HashMap<String, Vec<ContinentSummary>> = HashMap::new();
    for r in country_continent_rows(conn, &ids).await? {
        links
            .entry(r.country_id)
            .or_default()
            .push(ContinentSummary {
                id: r.id,
                name: r.name,
                kind: r.kind,
            });
    }
    Ok(rows
        .into_iter()
        .map(|r| Country {
            continents: links.remove(&r.id).unwrap_or_default(),
            id: r.id,
            name: r.name,
        })
        .collect())
}

pub(crate) async fn load_country(
    conn: &mut PgConnection,
    id: &str,
) -> sqlx::Result<Option<Country>> {
    let Some(row) = country_by_id(conn, id).await? else {
        return Ok(None);
    };
    Ok(load_countries(conn, vec![row]).await?.pop())
}

// ---------------------------------------------------------------- list SQL

/// The only sort of continents and countries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSort;

pub const SORTS: &[(&str, NameSort)] = &[("name", NameSort)];
pub const DEFAULT_SORT: Sort<NameSort> = Sort {
    key: NameSort,
    desc: false,
};

/// `type` values (spec §5.4).
pub const CONTINENT_TYPES: &[&str] = &["prehistoric", "modern"];

/// Continent list: optional `type`.
pub type ContinentQuery = ListQuery<Option<&'static str>, NameSort>;
/// Country list: optional continent (`?continent=`, `/continents/{id}/countries`).
pub type CountryQuery = ListQuery<Option<String>, NameSort>;

fn push_name_order(qb: &mut QueryBuilder<Postgres>, alias: &str, sort: Sort<NameSort>) {
    let dir = if sort.desc { " DESC" } else { "" };
    qb.push(format_args!(
        " ORDER BY lower({alias}.name) COLLATE paleo_name_sort{dir}, {alias}.id"
    ));
}

fn push_limit<F, S>(qb: &mut QueryBuilder<Postgres>, q: &ListQuery<F, S>) {
    qb.push(" LIMIT ")
        .push_bind(i64::from(q.page.limit))
        .push("::bigint OFFSET ")
        .push_bind(q.page.offset())
        .push("::bigint");
}

fn push_continent_where(qb: &mut QueryBuilder<Postgres>, kind: Option<&'static str>) {
    if let Some(kind) = kind {
        qb.push(" WHERE c.type = ").push_bind(kind).push("::text");
    }
}

/// Q-C1 / Q-C2 page statement.
pub fn push_continent_page_sql(qb: &mut QueryBuilder<Postgres>, q: &ContinentQuery) {
    qb.push("SELECT c.id, c.name, c.type AS kind FROM continents c");
    push_continent_where(qb, q.filter);
    push_name_order(qb, "c", q.sort);
    push_limit(qb, q);
}

/// Continent count statement.
pub fn push_continent_count_sql(qb: &mut QueryBuilder<Postgres>, q: &ContinentQuery) {
    qb.push("SELECT count(*) FROM continents c");
    push_continent_where(qb, q.filter);
}

/// Q-K1 / Q-C3 page statement.
pub fn push_country_page_sql(qb: &mut QueryBuilder<Postgres>, q: &CountryQuery) {
    match &q.filter {
        None => {
            qb.push("SELECT k.id, k.name FROM countries k");
        }
        Some(continent) => {
            qb.push(
                "SELECT k.id, k.name FROM countries k \
                 JOIN country_continents cc ON cc.country_id = k.id WHERE cc.continent_id = ",
            )
            .push_bind(continent.clone())
            .push("::text");
        }
    }
    push_name_order(qb, "k", q.sort);
    push_limit(qb, q);
}

/// Country count statement (no join).
pub fn push_country_count_sql(qb: &mut QueryBuilder<Postgres>, q: &CountryQuery) {
    match &q.filter {
        None => {
            qb.push("SELECT count(*) FROM countries k");
        }
        Some(continent) => {
            qb.push("SELECT count(*) FROM country_continents cc WHERE cc.continent_id = ")
                .push_bind(continent.clone())
                .push("::text");
        }
    }
}

// ---------------------------------------------------------------- read handlers

/// `GET /continents`.
pub async fn list_continents(
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let p = Params::parse(req.query_string(), &["page", "limit", "sort", "type"])?;
    let q = ContinentQuery {
        page: p.page()?,
        sort: p.sort(SORTS, DEFAULT_SORT)?,
        filter: p.one_of("type", CONTINENT_TYPES)?,
    };
    let mut tx = read_tx(&pool).await?;
    let (total, rows) = paged::<ContinentRow>(
        &mut tx,
        q.page,
        |qb| push_continent_count_sql(qb, &q),
        |qb| push_continent_page_sql(qb, &q),
    )
    .await?;
    tx.commit().await?;
    Ok(cached_json(
        &req,
        &Many {
            data: rows.into_iter().map(Continent::from).collect(),
            pagination: Pagination::new(q.page, total),
        },
    ))
}

/// `GET /continents/{continent_id}`.
pub async fn get_continent(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Continent, &id));
    }
    let mut conn = pool.acquire().await?;
    match continent_by_id(&mut conn, &id).await? {
        Some(row) => Ok(cached_json(
            &req,
            &One {
                data: Continent::from(row),
            },
        )),
        None => Err(ApiError::not_found(Resource::Continent, &id)),
    }
}

async fn list_countries_for(
    req: &HttpRequest,
    pool: &PgPool,
    q: CountryQuery,
    nested: bool,
) -> Result<HttpResponse, ApiError> {
    let mut tx = read_tx(pool).await?;
    if nested
        && let Some(continent) = &q.filter
        && continent_by_id(&mut tx, continent).await?.is_none()
    {
        return Err(ApiError::not_found(Resource::Continent, continent));
    }
    let (total, rows) = paged::<CountryRow>(
        &mut tx,
        q.page,
        |qb| push_country_count_sql(qb, &q),
        |qb| push_country_page_sql(qb, &q),
    )
    .await?;
    let data = load_countries(&mut tx, rows).await?;
    tx.commit().await?;
    Ok(cached_json(
        req,
        &Many {
            data,
            pagination: Pagination::new(q.page, total),
        },
    ))
}

/// `GET /countries`.
pub async fn list_countries(
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let p = Params::parse(req.query_string(), &["page", "limit", "sort", "continent"])?;
    let q = CountryQuery {
        page: p.page()?,
        sort: p.sort(SORTS, DEFAULT_SORT)?,
        filter: p.slug("continent")?,
    };
    list_countries_for(&req, &pool, q, false).await
}

/// `GET /continents/{continent_id}/countries`.
pub async fn list_continent_countries(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let p = Params::parse(req.query_string(), &["page", "limit", "sort"])?;
    let page = p.page()?;
    let sort = p.sort(SORTS, DEFAULT_SORT)?;
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Continent, &id));
    }
    let q = CountryQuery {
        page,
        sort,
        filter: Some(id),
    };
    list_countries_for(&req, &pool, q, true).await
}

/// `GET /countries/{country_id}`.
pub async fn get_country(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Country, &id));
    }
    let mut tx = read_tx(&pool).await?;
    let country = load_country(&mut tx, &id).await?;
    tx.commit().await?;
    match country {
        Some(data) => Ok(cached_json(&req, &One { data })),
        None => Err(ApiError::not_found(Resource::Country, &id)),
    }
}
