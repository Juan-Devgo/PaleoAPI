//! Species list SQL: filters and sorts (Q-S1…Q-S11, plan §Read path).

use sqlx::{PgConnection, Postgres, QueryBuilder};

use super::card::{SPECIES_COLUMNS, SpeciesRow};
use crate::api::params::{ListQuery, Sort};

/// Sortable fields (spec §5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeciesSort {
    Name,
    ScientificName,
    DiscoveryYear,
}

/// `sort` allow-list, in the order the error message lists them.
pub const SORTS: &[(&str, SpeciesSort)] = &[
    ("name", SpeciesSort::Name),
    ("scientific_name", SpeciesSort::ScientificName),
    ("discovery_year", SpeciesSort::DiscoveryYear),
];

pub const DEFAULT_SORT: Sort<SpeciesSort> = Sort {
    key: SpeciesSort::Name,
    desc: false,
};

/// Filters of `GET /species` (spec §5.6), combined with `AND`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeciesFilter {
    pub diet: Option<&'static str>,
    pub era: Option<String>,
    pub period: Option<String>,
    pub domain: Option<String>,
    pub kingdom: Option<String>,
    pub phylum: Option<String>,
    pub class: Option<String>,
    pub order: Option<String>,
    pub family: Option<String>,
    pub genus: Option<String>,
    pub continent: Option<String>,
    pub country: Option<String>,
    /// Escaped `ILIKE` pattern `%…%`.
    pub q: Option<String>,
}

pub type SpeciesQuery = ListQuery<SpeciesFilter, SpeciesSort>;

fn push_where(_qb: &mut QueryBuilder<Postgres>, _f: &SpeciesFilter) {}

fn push_order(qb: &mut QueryBuilder<Postgres>, sort: Sort<SpeciesSort>) {
    let dir = if sort.desc { " DESC" } else { "" };
    qb.push(" ORDER BY ");
    match sort.key {
        SpeciesSort::Name => qb.push(format_args!(
            "lower(s.name) COLLATE paleo_name_sort{dir}, s.id"
        )),
        SpeciesSort::ScientificName => qb.push(format_args!(
            "lower(s.scientific_name) COLLATE paleo_name_sort{dir}, s.id"
        )),
        SpeciesSort::DiscoveryYear => qb.push(format_args!(
            "s.discovery_year{} NULLS LAST, s.id",
            if sort.desc { " DESC" } else { " ASC" }
        )),
    };
}

/// Page statement: Q-S1…Q-S3 order, Q-S4…Q-S11 filters, `LIMIT`/`OFFSET`.
pub fn push_page_sql(qb: &mut QueryBuilder<Postgres>, q: &SpeciesQuery) {
    qb.push("SELECT ");
    qb.push(SPECIES_COLUMNS);
    qb.push(" FROM species s");
    push_where(qb, &q.filter);
    push_order(qb, q.sort);
    qb.push(" LIMIT ");
    qb.push_bind(i64::from(q.page.limit));
    qb.push("::bigint OFFSET ");
    qb.push_bind(q.page.offset());
    qb.push("::bigint");
}

/// Count statement with the same filters, no join, no order.
pub fn push_count_sql(qb: &mut QueryBuilder<Postgres>, q: &SpeciesQuery) {
    qb.push("SELECT count(*) FROM species s");
    push_where(qb, &q.filter);
}

/// Count first; the page query runs only when the page is not past the end.
pub async fn fetch(
    conn: &mut PgConnection,
    q: &SpeciesQuery,
) -> sqlx::Result<(i64, Vec<SpeciesRow>)> {
    crate::api::paged(
        conn,
        q.page,
        |qb| push_count_sql(qb, q),
        |qb| push_page_sql(qb, q),
    )
    .await
}
