//! Species (spec §5.6).

pub mod card;
pub mod input;
pub mod list;

use actix_web::{HttpRequest, HttpResponse, web};
use sqlx::PgPool;

use self::card::{SpeciesRow, load_cards};
use self::list::{DEFAULT_SORT, SORTS, SpeciesFilter, SpeciesQuery};
use super::error::{ApiError, Resource};
use super::http::{Many, One, cached_json};
use super::input::is_slug;
use super::params::{Pagination, Params};
use super::read_tx;

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
    pub const SPECIES_BY_ID_SQL = "SELECT s.id, s.name, s.scientific_name, s.diet, s.description, \
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
        filter: SpeciesFilter::default(),
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
