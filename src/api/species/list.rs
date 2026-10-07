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

/// Rank filters above genus (Q-S6): the rank's parent column on the topmost table and
/// the joins down to `genera`.
const RANK_FILTERS: [(&str, &str); 6] = [
    ("g.family_id", "genera g"),
    (
        "f.order_id",
        "families f JOIN genera g ON g.family_id = f.id",
    ),
    (
        "o.class_id",
        "orders o JOIN families f ON f.order_id = o.id JOIN genera g ON g.family_id = f.id",
    ),
    (
        "c.phylum_id",
        "classes c JOIN orders o ON o.class_id = c.id JOIN families f ON f.order_id = o.id \
         JOIN genera g ON g.family_id = f.id",
    ),
    (
        "p.kingdom_id",
        "phyla p JOIN classes c ON c.phylum_id = p.id JOIN orders o ON o.class_id = c.id \
         JOIN families f ON f.order_id = o.id JOIN genera g ON g.family_id = f.id",
    ),
    (
        "k.domain_id",
        "kingdoms k JOIN phyla p ON p.kingdom_id = k.id JOIN classes c ON c.phylum_id = p.id \
         JOIN orders o ON o.class_id = c.id JOIN families f ON f.order_id = o.id \
         JOIN genera g ON g.family_id = f.id",
    ),
];

/// Filters Q-S4…Q-S11, each bound, combined with `AND` (spec §4.8).
fn push_where(qb: &mut QueryBuilder<Postgres>, f: &SpeciesFilter) {
    let mut first = true;
    let mut and = |qb: &mut QueryBuilder<Postgres>| {
        qb.push(if first { " WHERE " } else { " AND " });
        first = false;
    };
    if let Some(diet) = f.diet {
        and(qb);
        qb.push("s.diet = ").push_bind(diet).push("::text");
    }
    if let Some(genus) = &f.genus {
        and(qb);
        qb.push("s.genus_id = ")
            .push_bind(genus.clone())
            .push("::text");
    }
    let ranks = [
        &f.family, &f.order, &f.class, &f.phylum, &f.kingdom, &f.domain,
    ];
    for (value, (column, from)) in ranks.into_iter().zip(RANK_FILTERS) {
        if let Some(id) = value {
            and(qb);
            // The genus set is computed once (an InitPlan), so a page that walks its sort
            // index tests each row against the set instead of probing the rank chain per
            // row (T059: `kingdom` pages took > 30 ms that way).
            qb.push(format_args!(
                "s.genus_id = ANY(ARRAY(SELECT g.id FROM {from} WHERE {column} = "
            ))
            .push_bind(id.clone())
            .push("::text))");
        }
    }
    if let Some(period) = &f.period {
        and(qb);
        qb.push(
            "EXISTS (SELECT 1 FROM species_periods sp \
             WHERE sp.species_id = s.id AND sp.period_id = ",
        )
        .push_bind(period.clone())
        .push("::text)");
    }
    if let Some(era) = &f.era {
        and(qb);
        qb.push(
            "EXISTS (SELECT 1 FROM species_periods sp \
             JOIN periods p ON p.id = sp.period_id AND p.era_id = ",
        )
        .push_bind(era.clone())
        .push("::text WHERE sp.species_id = s.id)");
    }
    if let Some(continent) = &f.continent {
        and(qb);
        qb.push(
            "EXISTS (SELECT 1 FROM species_continents sc \
             WHERE sc.species_id = s.id AND sc.continent_id = ",
        )
        .push_bind(continent.clone())
        .push("::text)");
    }
    if let Some(country) = &f.country {
        and(qb);
        qb.push(
            "EXISTS (SELECT 1 FROM species_countries sk \
             WHERE sk.species_id = s.id AND sk.country_id = ",
        )
        .push_bind(country.clone())
        .push("::text)");
    }
    if let Some(pattern) = &f.q {
        and(qb);
        qb.push("(s.name ILIKE ")
            .push_bind(pattern.clone())
            .push("::text ESCAPE '\\' OR s.scientific_name ILIKE ")
            .push_bind(pattern.clone())
            .push("::text ESCAPE '\\')");
    }
}

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
