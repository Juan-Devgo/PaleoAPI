//! The species card and its batched embeddings (spec §5.6, data-model §1, Q-S12).

use std::collections::HashMap;

use bigdecimal::BigDecimal;
use serde::Serialize;
use sqlx::PgConnection;

use crate::api::decimal::Decimal;

/// `{ "id", "name" }` of a related resource (spec §4.13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub id: String,
    pub name: String,
}

/// A continent summary also carries its `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContinentSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// One size measure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Range {
    pub min: Decimal,
    pub max: Decimal,
}

/// `size`; `null` as a whole when no measure is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Size {
    pub length_m: Option<Range>,
    pub height_m: Option<Range>,
    pub weight_kg: Option<Range>,
}

/// Full lineage, domain to genus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Taxonomy {
    pub domain: Summary,
    pub kingdom: Summary,
    pub phylum: Summary,
    pub class: Summary,
    pub order: Summary,
    pub family: Summary,
    pub genus: Summary,
}

/// A period with its era.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpeciesPeriod {
    pub id: String,
    pub name: String,
    pub era: Summary,
}

/// The card returned by list and detail alike (spec §5.6 response shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpeciesCard {
    pub id: String,
    pub name: String,
    pub scientific_name: String,
    pub diet: String,
    pub description: String,
    pub discovery_year: Option<i32>,
    pub image_url: Option<String>,
    pub size: Option<Size>,
    pub taxonomy: Taxonomy,
    pub periods: Vec<SpeciesPeriod>,
    pub continents: Vec<ContinentSummary>,
    pub countries: Vec<Summary>,
}

/// The `species` columns of a card (page SQL and Q-S13).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SpeciesRow {
    pub id: String,
    pub name: String,
    pub scientific_name: String,
    pub diet: String,
    pub description: String,
    pub discovery_year: Option<i32>,
    pub image_url: Option<String>,
    pub length_min_m: Option<BigDecimal>,
    pub length_max_m: Option<BigDecimal>,
    pub height_min_m: Option<BigDecimal>,
    pub height_max_m: Option<BigDecimal>,
    pub weight_min_kg: Option<BigDecimal>,
    pub weight_max_kg: Option<BigDecimal>,
}

/// The column list of [`SpeciesRow`], qualified with alias `s`.
pub const SPECIES_COLUMNS: &str = "s.id, s.name, s.scientific_name, s.diet, s.description, \
     s.discovery_year, s.image_url, s.length_min_m, s.length_max_m, s.height_min_m, \
     s.height_max_m, s.weight_min_kg, s.weight_max_kg";

#[derive(Debug)]
struct TaxonomyRow {
    species_id: String,
    domain_id: String,
    domain_name: String,
    kingdom_id: String,
    kingdom_name: String,
    phylum_id: String,
    phylum_name: String,
    class_id: String,
    class_name: String,
    order_id: String,
    order_name: String,
    family_id: String,
    family_name: String,
    genus_id: String,
    genus_name: String,
}

#[derive(Debug)]
struct PeriodRow {
    species_id: String,
    id: String,
    name: String,
    era_id: String,
    era_name: String,
}

#[derive(Debug)]
struct ContinentRow {
    species_id: String,
    id: String,
    name: String,
    kind: String,
}

#[derive(Debug)]
struct CountryRow {
    species_id: String,
    id: String,
    name: String,
}

fixed_query! {
    /// Q-S12: lineage of a page of species, PK lookups from `genera` up to `domains`.
    pub const SPECIES_TAXONOMY_SQL = "SELECT s.id AS species_id, \
            d.id AS domain_id, d.name AS domain_name, k.id AS kingdom_id, k.name AS kingdom_name, \
            p.id AS phylum_id, p.name AS phylum_name, c.id AS class_id, c.name AS class_name, \
            o.id AS order_id, o.name AS order_name, f.id AS family_id, f.name AS family_name, \
            g.id AS genus_id, g.name AS genus_name \
        FROM species s \
        JOIN genera g ON g.id = s.genus_id \
        JOIN families f ON f.id = g.family_id \
        JOIN orders o ON o.id = f.order_id \
        JOIN classes c ON c.id = o.class_id \
        JOIN phyla p ON p.id = c.phylum_id \
        JOIN kingdoms k ON k.id = p.kingdom_id \
        JOIN domains d ON d.id = k.domain_id \
        WHERE s.id = ANY($1::text[])";
    fn taxonomy_rows(ids: &[String]) -> fetch_all TaxonomyRow;
}

fixed_query! {
    /// Q-S12: periods of a page of species with their era, oldest first.
    pub const SPECIES_PERIODS_SQL = "SELECT sp.species_id, p.id, p.name, e.id AS era_id, e.name AS era_name \
        FROM species_periods sp \
        JOIN periods p ON p.id = sp.period_id \
        JOIN eras e ON e.id = p.era_id \
        WHERE sp.species_id = ANY($1::text[]) \
        ORDER BY p.start_mya DESC, p.id";
    fn period_rows(ids: &[String]) -> fetch_all PeriodRow;
}

fixed_query! {
    /// Q-S12: continents of a page of species, prehistoric first, then by name.
    pub const SPECIES_CONTINENTS_SQL = "SELECT sc.species_id, c.id, c.name, c.type AS kind \
        FROM species_continents sc \
        JOIN continents c ON c.id = sc.continent_id \
        WHERE sc.species_id = ANY($1::text[]) \
        ORDER BY (c.type = 'modern'), lower(c.name) COLLATE paleo_name_sort, c.id";
    fn continent_rows(ids: &[String]) -> fetch_all ContinentRow;
}

fixed_query! {
    /// Q-S12: countries of a page of species, by name.
    pub const SPECIES_COUNTRIES_SQL = "SELECT sc.species_id, c.id, c.name \
        FROM species_countries sc \
        JOIN countries c ON c.id = sc.country_id \
        WHERE sc.species_id = ANY($1::text[]) \
        ORDER BY lower(c.name) COLLATE paleo_name_sort, c.id";
    fn country_rows(ids: &[String]) -> fetch_all CountryRow;
}

fn range(min: Option<BigDecimal>, max: Option<BigDecimal>) -> Option<Range> {
    Some(Range {
        min: Decimal::from_db(min?),
        max: Decimal::from_db(max?),
    })
}

fn size(row: &mut SpeciesRow) -> Option<Size> {
    let size = Size {
        length_m: range(row.length_min_m.take(), row.length_max_m.take()),
        height_m: range(row.height_min_m.take(), row.height_max_m.take()),
        weight_kg: range(row.weight_min_kg.take(), row.weight_max_kg.take()),
    };
    (size.length_m.is_some() || size.height_m.is_some() || size.weight_kg.is_some()).then_some(size)
}

fn group<R, T>(
    rows: Vec<R>,
    key: impl Fn(&R) -> &str,
    map: impl Fn(R) -> T,
) -> HashMap<String, Vec<T>> {
    let mut out: HashMap<String, Vec<T>> = HashMap::new();
    for r in rows {
        let k = key(&r).to_string();
        out.entry(k).or_default().push(map(r));
    }
    out
}

/// Builds the cards of `rows` (in order) with four statements, whatever the page size
/// (plan §Read path Embedding). Run inside the read transaction (research R8).
pub async fn load_cards(
    conn: &mut PgConnection,
    rows: Vec<SpeciesRow>,
) -> sqlx::Result<Vec<SpeciesCard>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();

    let mut lineage: HashMap<String, Taxonomy> = taxonomy_rows(conn, &ids)
        .await?
        .into_iter()
        .map(|t| {
            let s = |id: String, name: String| Summary { id, name };
            (
                t.species_id,
                Taxonomy {
                    domain: s(t.domain_id, t.domain_name),
                    kingdom: s(t.kingdom_id, t.kingdom_name),
                    phylum: s(t.phylum_id, t.phylum_name),
                    class: s(t.class_id, t.class_name),
                    order: s(t.order_id, t.order_name),
                    family: s(t.family_id, t.family_name),
                    genus: s(t.genus_id, t.genus_name),
                },
            )
        })
        .collect();
    let mut periods = group(
        period_rows(conn, &ids).await?,
        |r| &r.species_id,
        |r| SpeciesPeriod {
            id: r.id,
            name: r.name,
            era: Summary {
                id: r.era_id,
                name: r.era_name,
            },
        },
    );
    let mut continents = group(
        continent_rows(conn, &ids).await?,
        |r| &r.species_id,
        |r| ContinentSummary {
            id: r.id,
            name: r.name,
            kind: r.kind,
        },
    );
    let mut countries = group(
        country_rows(conn, &ids).await?,
        |r| &r.species_id,
        |r| Summary {
            id: r.id,
            name: r.name,
        },
    );

    let mut cards = Vec::with_capacity(rows.len());
    for mut row in rows {
        // Every species has a genus (NOT NULL FK), so the lineage row exists in this snapshot.
        let Some(taxonomy) = lineage.remove(&row.id) else {
            return Err(sqlx::Error::RowNotFound);
        };
        let size = size(&mut row);
        cards.push(SpeciesCard {
            periods: periods.remove(&row.id).unwrap_or_default(),
            continents: continents.remove(&row.id).unwrap_or_default(),
            countries: countries.remove(&row.id).unwrap_or_default(),
            id: row.id,
            name: row.name,
            scientific_name: row.scientific_name,
            diet: row.diet,
            description: row.description,
            discovery_year: row.discovery_year,
            image_url: row.image_url,
            size,
            taxonomy,
        });
    }
    Ok(cards)
}
