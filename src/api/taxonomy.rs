//! Taxonomy ranks (spec §5.3): seven tables with one shape, described by [`Rank`].
//!
//! Statements are static strings built per rank with `concat!` (research R3) and run
//! with runtime `query_as`; `query_plans.rs` `EXPLAIN`s every one of them.

use actix_web::{HttpRequest, HttpResponse, web};
use serde::ser::{Serialize, SerializeMap, Serializer};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::error::{ApiError, Resource};
use super::http::{Many, One, cached_json};
use super::input::is_slug;
use super::params::{ListQuery, Pagination, Params, Sort};
use super::species::card::Summary;
use super::{paged, read_tx};

/// One taxonomy rank.
#[derive(Debug)]
pub struct Rank {
    /// Table name and plural path segment.
    pub plural: &'static str,
    /// Response key of this rank as a parent, and the filter parameter of its children.
    pub singular: &'static str,
    pub resource: Resource,
    /// Index of the parent rank in [`RANKS`].
    pub parent: Option<usize>,
    /// Index of the child rank in [`RANKS`].
    pub child: Option<usize>,
    /// Parent column (`domain_id`), `None` for domains.
    pub parent_col: Option<&'static str>,
    /// Q-T3: one item by id with its parent summary (PK join).
    pub by_id_sql: &'static str,
    /// Q-W3: whether an item exists (nested lists and creates).
    pub exists_sql: &'static str,
    /// Q-T1 / Q-T2 page statement prefix: `SELECT … FROM <rank> t [JOIN <parent> p …]`.
    pub page_select: &'static str,
    /// Count statement prefix (no join).
    pub count_select: &'static str,
    /// Q-W10: dependents (child rows, or species of a genus), counted, first 5 ids.
    pub dependents_sql: &'static str,
    /// Noun of the dependents in messages: (singular, plural).
    pub dependents_noun: (&'static str, &'static str),
}

/// Builds a [`Rank`]; `$parent` is `(index, table, column)` for every rank but domains.
macro_rules! rank {
    ($plural:literal, $singular:literal, $res:ident, child: $child:expr,
     dependents: ($dtable:literal, $dcol:literal, $dnoun:literal, $dnouns:literal)) => {
        Rank {
            plural: $plural,
            singular: $singular,
            resource: Resource::$res,
            parent: None,
            child: $child,
            parent_col: None,
            by_id_sql: concat!(
                "SELECT t.id, t.name, NULL::text AS parent_id, NULL::text AS parent_name FROM ",
                $plural,
                " t WHERE t.id = $1"
            ),
            exists_sql: concat!("SELECT 1 FROM ", $plural, " WHERE id = $1"),
            page_select: concat!(
                "SELECT t.id, t.name, NULL::text AS parent_id, NULL::text AS parent_name FROM ",
                $plural,
                " t"
            ),
            count_select: concat!("SELECT count(*) FROM ", $plural, " t"),
            dependents_sql: concat!(
                "SELECT c.id, count(*) OVER () AS total FROM ",
                $dtable,
                " c WHERE c.",
                $dcol,
                " = $1 ORDER BY c.id LIMIT 5"
            ),
            dependents_noun: ($dnoun, $dnouns),
        }
    };
    ($plural:literal, $singular:literal, $res:ident, parent: ($pidx:expr, $ptable:literal, $pcol:literal),
     child: $child:expr, dependents: ($dtable:literal, $dcol:literal, $dnoun:literal, $dnouns:literal)) => {
        Rank {
            plural: $plural,
            singular: $singular,
            resource: Resource::$res,
            parent: Some($pidx),
            child: $child,
            parent_col: Some($pcol),
            by_id_sql: concat!(
                "SELECT t.id, t.name, p.id AS parent_id, p.name AS parent_name FROM ",
                $plural,
                " t JOIN ",
                $ptable,
                " p ON p.id = t.",
                $pcol,
                " WHERE t.id = $1"
            ),
            exists_sql: concat!("SELECT 1 FROM ", $plural, " WHERE id = $1"),
            page_select: concat!(
                "SELECT t.id, t.name, p.id AS parent_id, p.name AS parent_name FROM ",
                $plural,
                " t JOIN ",
                $ptable,
                " p ON p.id = t.",
                $pcol
            ),
            count_select: concat!("SELECT count(*) FROM ", $plural, " t"),
            dependents_sql: concat!(
                "SELECT c.id, count(*) OVER () AS total FROM ",
                $dtable,
                " c WHERE c.",
                $dcol,
                " = $1 ORDER BY c.id LIMIT 5"
            ),
            dependents_noun: ($dnoun, $dnouns),
        }
    };
}

/// The seven ranks, top-down (spec §5.3).
pub static RANKS: [Rank; 7] = [
    rank!("domains", "domain", Domain, child: Some(1),
        dependents: ("kingdoms", "domain_id", "kingdom", "kingdoms")),
    rank!("kingdoms", "kingdom", Kingdom, parent: (0, "domains", "domain_id"), child: Some(2),
        dependents: ("phyla", "kingdom_id", "phylum", "phyla")),
    rank!("phyla", "phylum", Phylum, parent: (1, "kingdoms", "kingdom_id"), child: Some(3),
        dependents: ("classes", "phylum_id", "class", "classes")),
    rank!("classes", "class", Class, parent: (2, "phyla", "phylum_id"), child: Some(4),
        dependents: ("orders", "class_id", "order", "orders")),
    rank!("orders", "order", Order, parent: (3, "classes", "class_id"), child: Some(5),
        dependents: ("families", "order_id", "family", "families")),
    rank!("families", "family", Family, parent: (4, "orders", "order_id"), child: Some(6),
        dependents: ("genera", "family_id", "genus", "genera")),
    rank!("genera", "genus", Genus, parent: (5, "families", "family_id"), child: None,
        dependents: ("species", "genus_id", "species", "species")),
];

impl Rank {
    /// The rank of a taxonomy resource.
    pub fn of(res: Resource) -> Option<&'static Rank> {
        RANKS.iter().find(|r| r.resource == res)
    }
    pub fn parent_rank(&self) -> Option<&'static Rank> {
        self.parent.map(|i| &RANKS[i])
    }
    pub fn child_rank(&self) -> Option<&'static Rank> {
        self.child.map(|i| &RANKS[i])
    }
}

/// A rank row (page statement and Q-T3).
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RankRow {
    id: String,
    name: String,
    parent_id: Option<String>,
    parent_name: Option<String>,
}

/// A rank item: `id`, `name`, and the parent summary under the parent's singular key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankItem {
    pub id: String,
    pub name: String,
    pub parent: Option<(&'static str, Summary)>,
}

impl Serialize for RankItem {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(2 + usize::from(self.parent.is_some())))?;
        m.serialize_entry("id", &self.id)?;
        m.serialize_entry("name", &self.name)?;
        if let Some((key, summary)) = &self.parent {
            m.serialize_entry(key, summary)?;
        }
        m.end()
    }
}

impl RankItem {
    fn from_row(rank: &Rank, r: RankRow) -> Self {
        let parent = match (rank.parent_rank(), r.parent_id, r.parent_name) {
            (Some(p), Some(id), Some(name)) => Some((p.singular, Summary { id, name })),
            _ => None,
        };
        Self {
            id: r.id,
            name: r.name,
            parent,
        }
    }
}

/// The only sort of ranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSort;

pub const SORTS: &[(&str, NameSort)] = &[("name", NameSort)];
pub const DEFAULT_SORT: Sort<NameSort> = Sort {
    key: NameSort,
    desc: false,
};

/// A rank list: optional parent id (`?<parent>=` or the nested path).
pub type RankQuery = ListQuery<Option<String>, NameSort>;

fn push_where(qb: &mut QueryBuilder<Postgres>, rank: &Rank, parent: &Option<String>) {
    if let (Some(col), Some(id)) = (rank.parent_col, parent) {
        qb.push(format_args!(" WHERE t.{col} = "))
            .push_bind(id.clone())
            .push("::text");
    }
}

/// Q-T1 / Q-T2 page statement, parent joined on its PK.
pub fn push_page_sql(qb: &mut QueryBuilder<Postgres>, rank: &Rank, q: &RankQuery) {
    qb.push(rank.page_select);
    push_where(qb, rank, &q.filter);
    qb.push(if q.sort.desc {
        " ORDER BY lower(t.name) COLLATE paleo_name_sort DESC, t.id"
    } else {
        " ORDER BY lower(t.name) COLLATE paleo_name_sort, t.id"
    });
    qb.push(" LIMIT ")
        .push_bind(i64::from(q.page.limit))
        .push("::bigint OFFSET ")
        .push_bind(q.page.offset())
        .push("::bigint");
}

/// Count statement (no join).
pub fn push_count_sql(qb: &mut QueryBuilder<Postgres>, rank: &Rank, q: &RankQuery) {
    qb.push(rank.count_select);
    push_where(qb, rank, &q.filter);
}

pub(crate) async fn rank_by_id(
    conn: &mut sqlx::PgConnection,
    rank: &Rank,
    id: &str,
) -> sqlx::Result<Option<RankItem>> {
    let row = sqlx::query_as::<_, RankRow>(rank.by_id_sql)
        .bind(id)
        .fetch_optional(conn)
        .await?;
    Ok(row.map(|r| RankItem::from_row(rank, r)))
}

pub(crate) async fn rank_exists(
    conn: &mut sqlx::PgConnection,
    rank: &Rank,
    id: &str,
) -> sqlx::Result<bool> {
    Ok(sqlx::query(rank.exists_sql)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .is_some())
}

// ---------------------------------------------------------------- read handlers

async fn run_list(
    req: &HttpRequest,
    pool: &PgPool,
    rank: &'static Rank,
    q: RankQuery,
    nested_parent: Option<&'static Rank>,
) -> Result<HttpResponse, ApiError> {
    let mut tx = read_tx(pool).await?;
    if let (Some(parent), Some(id)) = (nested_parent, &q.filter)
        && !rank_exists(&mut tx, parent, id).await?
    {
        return Err(ApiError::not_found(parent.resource, id));
    }
    let (total, rows) = paged::<RankRow>(
        &mut tx,
        q.page,
        |qb| push_count_sql(qb, rank, &q),
        |qb| push_page_sql(qb, rank, &q),
    )
    .await?;
    tx.commit().await?;
    Ok(cached_json(
        req,
        &Many {
            data: rows
                .into_iter()
                .map(|r| RankItem::from_row(rank, r))
                .collect(),
            pagination: Pagination::new(q.page, total),
        },
    ))
}

/// `GET /taxonomy/{rank}` with the optional `?<parent>=` filter.
pub async fn list(
    rank: &'static Rank,
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let known: Vec<&'static str> = ["page", "limit", "sort"]
        .into_iter()
        .chain(rank.parent_rank().map(|p| p.singular))
        .collect();
    let p = Params::parse(req.query_string(), &known)?;
    let q = RankQuery {
        page: p.page()?,
        sort: p.sort(SORTS, DEFAULT_SORT)?,
        filter: match rank.parent_rank() {
            Some(parent) => p.slug(parent.singular)?,
            None => None,
        },
    };
    run_list(&req, &pool, rank, q, None).await
}

/// `GET /taxonomy/{parent_rank}/{parent_id}/{rank}`.
pub async fn list_children(
    rank: &'static Rank,
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let parent = rank
        .parent_rank()
        .expect("nested routes exist only below domains");
    let p = Params::parse(req.query_string(), &["page", "limit", "sort"])?;
    let page = p.page()?;
    let sort = p.sort(SORTS, DEFAULT_SORT)?;
    let parent_id = path.into_inner();
    if !is_slug(&parent_id) {
        return Err(ApiError::not_found(parent.resource, &parent_id));
    }
    let q = RankQuery {
        page,
        sort,
        filter: Some(parent_id),
    };
    run_list(&req, &pool, rank, q, Some(parent)).await
}

/// `GET /taxonomy/{rank}/{id}`.
pub async fn detail(
    rank: &'static Rank,
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(rank.resource, &id));
    }
    let mut conn = pool.acquire().await?;
    match rank_by_id(&mut conn, rank, &id).await? {
        Some(data) => Ok(cached_json(&req, &One { data })),
        None => Err(ApiError::not_found(rank.resource, &id)),
    }
}
