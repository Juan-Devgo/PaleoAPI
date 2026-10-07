//! Database errors → responses (data-model §4) and delete dependents (data-model §5).
//!
//! Errors are mapped by SQLSTATE plus constraint name only; `DETAIL`, `MESSAGE`, and
//! `HINT` are never forwarded (001 data-model §8).

use sqlx::PgConnection;

use super::error::{ApiError, FieldError, Resource};
use super::geologic_time::{Sent, overlap_field};
use super::input::truncate;
use super::taxonomy::Rank;

// ---------------------------------------------------------------- the map

/// What a constraint (or trigger-raised name) means for a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// `23505` on an id: `409 <R>_ALREADY_EXISTS`.
    DuplicateId(Resource),
    /// `23505` on a case-insensitive unique name: `409 <R>_ALREADY_EXISTS`.
    DuplicateName(Resource, &'static str),
    /// `23505` on a link table: `422` on the list field.
    DuplicateLink(&'static str),
    /// `23503`: `422` on the field for inserts/updates, `409` dependents for deletes.
    ForeignKey(&'static str),
    /// `23514` on one column: `422` on its field.
    Check(&'static str),
    /// `23514`/`23P01` on a multi-field range rule: `422` per data-model §6.
    Range(RangeRule),
    /// Deferred minimum link counts, raised at `COMMIT`: `422` on the list field.
    MinLinks(&'static str),
    /// Writes the API never issues: `500`.
    Internal,
}

/// The multi-field range rules (data-model §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeRule {
    Order,
    Overlap,
    WithinEra,
    ContainsPeriods,
}

/// Resource tables and their resource.
const RESOURCE_TABLES: [(&str, Resource); 12] = [
    ("eras", Resource::Era),
    ("periods", Resource::Period),
    ("domains", Resource::Domain),
    ("kingdoms", Resource::Kingdom),
    ("phyla", Resource::Phylum),
    ("classes", Resource::Class),
    ("orders", Resource::Order),
    ("families", Resource::Family),
    ("genera", Resource::Genus),
    ("continents", Resource::Continent),
    ("countries", Resource::Country),
    ("species", Resource::Species),
];

/// Link tables (they have a truncate guard too).
const LINK_TABLES: [&str; 4] = [
    "country_continents",
    "species_periods",
    "species_continents",
    "species_countries",
];

/// Constraints with a fixed meaning.
const NAMED: &[(&str, Rule)] = &[
    (
        "continents_id_type_uq",
        Rule::DuplicateId(Resource::Continent),
    ),
    (
        "species_scientific_name_uq",
        Rule::DuplicateName(Resource::Species, "scientific_name"),
    ),
    (
        "country_continents_pk",
        Rule::DuplicateLink("continent_ids"),
    ),
    ("species_periods_pk", Rule::DuplicateLink("period_ids")),
    (
        "species_continents_pk",
        Rule::DuplicateLink("continent_ids"),
    ),
    ("species_countries_pk", Rule::DuplicateLink("country_ids")),
    ("periods_era_fk", Rule::ForeignKey("era_id")),
    ("kingdoms_domain_fk", Rule::ForeignKey("domain_id")),
    ("phyla_kingdom_fk", Rule::ForeignKey("kingdom_id")),
    ("classes_phylum_fk", Rule::ForeignKey("phylum_id")),
    ("orders_class_fk", Rule::ForeignKey("class_id")),
    ("families_order_fk", Rule::ForeignKey("order_id")),
    ("genera_family_fk", Rule::ForeignKey("family_id")),
    ("species_genus_fk", Rule::ForeignKey("genus_id")),
    ("country_continents_country_fk", Rule::ForeignKey("id")),
    (
        "country_continents_continent_fk",
        Rule::ForeignKey("continent_ids"),
    ),
    ("species_periods_species_fk", Rule::ForeignKey("id")),
    ("species_continents_species_fk", Rule::ForeignKey("id")),
    ("species_countries_species_fk", Rule::ForeignKey("id")),
    ("species_periods_period_fk", Rule::ForeignKey("period_ids")),
    (
        "species_continents_continent_fk",
        Rule::ForeignKey("continent_ids"),
    ),
    (
        "species_countries_country_fk",
        Rule::ForeignKey("country_ids"),
    ),
    ("species_scientific_name_ck", Rule::Check("scientific_name")),
    ("species_description_ck", Rule::Check("description")),
    ("species_image_url_ck", Rule::Check("image_url")),
    ("species_diet_ck", Rule::Check("diet")),
    ("continents_type_ck", Rule::Check("type")),
    ("species_discovery_year_ck", Rule::Check("discovery_year")),
    (
        "species_discovery_year_not_future_ck",
        Rule::Check("discovery_year"),
    ),
    ("eras_start_mya_ck", Rule::Check("start_mya")),
    ("periods_start_mya_ck", Rule::Check("start_mya")),
    ("eras_end_mya_ck", Rule::Check("end_mya")),
    ("periods_end_mya_ck", Rule::Check("end_mya")),
    ("species_length_ck", Rule::Check("size.length_m")),
    ("species_height_ck", Rule::Check("size.height_m")),
    ("species_weight_ck", Rule::Check("size.weight_kg")),
    ("eras_range_ck", Rule::Range(RangeRule::Order)),
    ("periods_range_ck", Rule::Range(RangeRule::Order)),
    ("eras_range_ex", Rule::Range(RangeRule::Overlap)),
    ("periods_range_ex", Rule::Range(RangeRule::Overlap)),
    ("periods_within_era_ck", Rule::Range(RangeRule::WithinEra)),
    (
        "eras_contains_periods_ck",
        Rule::Range(RangeRule::ContainsPeriods),
    ),
    (
        "countries_min_continents_ck",
        Rule::MinLinks("continent_ids"),
    ),
    ("species_min_periods_ck", Rule::MinLinks("period_ids")),
    ("country_continents_continent_type_ck", Rule::Internal),
];

/// The rule of a constraint, unique index, or trigger-raised name.
pub fn classify(name: &str) -> Option<Rule> {
    if let Some((_, rule)) = NAMED.iter().find(|(n, _)| *n == name) {
        return Some(*rule);
    }
    for (table, res) in RESOURCE_TABLES {
        let Some(rule) = name
            .strip_prefix(table)
            .and_then(|rest| rest.strip_prefix('_'))
        else {
            continue;
        };
        match rule {
            "pk" => return Some(Rule::DuplicateId(res)),
            "id_ck" => return Some(Rule::Check("id")),
            "name_ck" => return Some(Rule::Check("name")),
            "name_uq" if res != Resource::Species => {
                return Some(Rule::DuplicateName(res, "name"));
            }
            "id_immutable_ck" | "no_truncate_ck" => return Some(Rule::Internal),
            _ => {}
        }
    }
    LINK_TABLES
        .iter()
        .any(|t| name.strip_prefix(t) == Some("_no_truncate_ck"))
        .then_some(Rule::Internal)
}

/// Whether `constraint` (a constraint, unique index, or trigger-raised name) has a row in the error map.
pub fn is_mapped(constraint: &str) -> bool {
    classify(constraint).is_some()
}

// ---------------------------------------------------------------- mapping

/// The statement kind that failed (data-model §4 "Statement").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Insert,
    Update,
    /// A continent `UPDATE` (its type change is guarded by `country_continents_continent_fk`).
    ContinentUpdate,
}

/// What the failed write was about, for messages and field attribution.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    pub res: Resource,
    pub id: &'a str,
    pub op: Op,
    /// The name (or scientific name) the write tried to store.
    pub name: Option<&'a str>,
    /// Range fields sent (data-model §6).
    pub sent: Sent,
}

impl<'a> Ctx<'a> {
    pub fn new(res: Resource, id: &'a str, op: Op) -> Self {
        Self {
            res,
            id,
            op,
            name: None,
            sent: Sent::default(),
        }
    }
    pub fn name(mut self, name: Option<&'a str>) -> Self {
        self.name = name;
        self
    }
    pub fn sent(mut self, sent: Sent) -> Self {
        self.sent = sent;
        self
    }
}

const ECHO: usize = 32;

fn one(field: &str, message: impl Into<String>) -> ApiError {
    ApiError::validation(vec![FieldError::new(field, message)])
}

/// Maps an error of a write statement or its `COMMIT` (data-model §4).
pub fn map_write(e: &sqlx::Error, ctx: &Ctx<'_>) -> ApiError {
    let Some(db) = e.as_database_error() else {
        return internal(e);
    };
    let code = db.code();
    let rule = db.constraint().and_then(classify);
    match (code.as_deref(), rule) {
        (Some("23505"), Some(Rule::DuplicateId(res))) => ApiError::already_exists(
            res,
            format!(
                "{} {} with id '{}' already exists. Choose another id.",
                capitalize(res.article()),
                res.noun(),
                truncate(ctx.id, 64)
            ),
        ),
        (Some("23505"), Some(Rule::DuplicateName(res, field))) => {
            let name = truncate(ctx.name.unwrap_or_default(), ECHO);
            let message = if field == "name" {
                format!(
                    "{} {} named '{name}' already exists (names are compared ignoring case).",
                    capitalize(res.article()),
                    res.noun()
                )
            } else {
                format!(
                    "{} {} with {field} '{name}' already exists (compared ignoring case).",
                    capitalize(res.article()),
                    res.noun()
                )
            };
            ApiError::already_exists(res, message)
        }
        (Some("23505"), Some(Rule::DuplicateLink(field))) => {
            one(field, format!("{field} lists an id more than once."))
        }
        (Some("23503"), Some(Rule::ForeignKey("continent_ids")))
            if ctx.op == Op::ContinentUpdate =>
        {
            one(
                "type",
                "type cannot change to prehistoric while countries are linked to this continent.",
            )
        }
        (Some("23503"), Some(Rule::ForeignKey(field))) if field != "id" => one(
            field,
            if field == "continent_ids" && ctx.res == Resource::Country {
                "continent_ids must list existing modern continents.".to_string()
            } else {
                format!("{field} references a record that does not exist.")
            },
        ),
        (Some("23514"), Some(Rule::Check(field))) => one(
            field,
            format!("{field} is not valid. Check the rules for this field."),
        ),
        (Some("23514" | "23P01"), Some(Rule::Range(rule))) => one(
            range_field(rule, ctx.sent),
            match rule {
                RangeRule::Order => "start_mya must be greater than end_mya.",
                RangeRule::Overlap => {
                    "The range overlaps another one; ranges may touch but not overlap."
                }
                RangeRule::WithinEra => "A period's range must lie within its era's range.",
                RangeRule::ContainsPeriods => "An era's range must contain all of its periods.",
            },
        ),
        (Some("23514"), Some(Rule::MinLinks(field))) => {
            one(field, format!("{field} must keep at least one item."))
        }
        _ => internal(e),
    }
}

/// Field of a range rule raised by the database (data-model §6).
fn range_field(rule: RangeRule, sent: Sent) -> &'static str {
    let only_start = sent.start_mya && !sent.end_mya;
    let only_end = sent.end_mya && !sent.start_mya;
    match rule {
        RangeRule::Order if only_start => "start_mya",
        RangeRule::Order => "end_mya",
        RangeRule::Overlap => overlap_field(sent),
        RangeRule::WithinEra if sent.era_id && !sent.start_mya && !sent.end_mya => "era_id",
        RangeRule::WithinEra | RangeRule::ContainsPeriods if only_end => "end_mya",
        RangeRule::WithinEra | RangeRule::ContainsPeriods => "start_mya",
    }
}

/// Whether a delete failed because rows still reference the target.
pub fn is_fk_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|c| c == "23503")
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// `500 INTERNAL_ERROR` after one stderr line with SQLSTATE and constraint only
/// (plan §Write path). Never logs messages, details, or values.
pub fn internal(e: &sqlx::Error) -> ApiError {
    match e.as_database_error() {
        Some(db) => eprintln!(
            "paleo_api: internal error: {} {}",
            db.code().as_deref().unwrap_or("-"),
            db.constraint().unwrap_or("-")
        ),
        None => eprintln!("paleo_api: internal error: {}", kind(e)),
    }
    ApiError::internal()
}

/// A short, value-free name for a non-database error.
fn kind(e: &sqlx::Error) -> &'static str {
    match e {
        sqlx::Error::PoolTimedOut => "pool-timed-out",
        sqlx::Error::PoolClosed => "pool-closed",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::Tls(_) => "tls",
        sqlx::Error::Protocol(_) => "protocol",
        sqlx::Error::RowNotFound => "row-not-found",
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => "decode",
        _ => "driver",
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        internal(&e)
    }
}

// ---------------------------------------------------------------- dependents (data-model §5)

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct DependentRow {
    id: String,
    total: i64,
}

fixed_query! {
    /// Q-W10: periods of an era.
    pub const ERA_DEPENDENTS_SQL = "SELECT p.id, count(*) OVER () AS \"total!\" FROM periods p \
        WHERE p.era_id = $1 ORDER BY p.id LIMIT 5";
    fn era_dependents(id: &str) -> fetch_all DependentRow;
}

fixed_query! {
    /// Q-W10: species linked to a period.
    pub const PERIOD_DEPENDENTS_SQL = "SELECT sp.species_id AS id, count(*) OVER () AS \"total!\" \
        FROM species_periods sp WHERE sp.period_id = $1 ORDER BY sp.species_id LIMIT 5";
    fn period_dependents(id: &str) -> fetch_all DependentRow;
}

fixed_query! {
    /// Q-W9 / Q-W10: countries linked to a continent.
    pub const CONTINENT_COUNTRIES_SQL = "SELECT cc.country_id AS id, count(*) OVER () AS \"total!\" \
        FROM country_continents cc WHERE cc.continent_id = $1 ORDER BY cc.country_id LIMIT 5";
    pub(crate) fn continent_countries(id: &str) -> fetch_all DependentRow;
}

fixed_query! {
    /// Q-W10: species linked to a continent.
    pub const CONTINENT_SPECIES_SQL = "SELECT sc.species_id AS id, count(*) OVER () AS \"total!\" \
        FROM species_continents sc WHERE sc.continent_id = $1 ORDER BY sc.species_id LIMIT 5";
    fn continent_species(id: &str) -> fetch_all DependentRow;
}

fixed_query! {
    /// Q-W10: species linked to a country.
    pub const COUNTRY_DEPENDENTS_SQL = "SELECT sk.species_id AS id, count(*) OVER () AS \"total!\" \
        FROM species_countries sk WHERE sk.country_id = $1 ORDER BY sk.species_id LIMIT 5";
    fn country_dependents(id: &str) -> fetch_all DependentRow;
}

/// One kind of dependent: how many, and the first ids by `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependentKind {
    pub count: i64,
    pub singular: &'static str,
    pub plural: &'static str,
    pub ids: Vec<String>,
}

impl DependentKind {
    fn from_rows(
        rows: Vec<DependentRow>,
        singular: &'static str,
        plural: &'static str,
    ) -> Option<Self> {
        let count = rows.first()?.total;
        Some(Self {
            count,
            singular,
            plural,
            ids: rows.into_iter().map(|r| r.id).collect(),
        })
    }

    fn describe(&self) -> String {
        let noun = if self.count == 1 {
            self.singular
        } else {
            self.plural
        };
        format!("{} {noun}", self.count)
    }
}

/// The dependents that block deleting `res` `id`; empty when the delete may proceed.
pub async fn dependents(
    conn: &mut PgConnection,
    res: Resource,
    id: &str,
) -> sqlx::Result<Vec<DependentKind>> {
    let kinds = match res {
        Resource::Era => vec![DependentKind::from_rows(
            era_dependents(conn, id).await?,
            "period",
            "periods",
        )],
        Resource::Period => vec![DependentKind::from_rows(
            period_dependents(conn, id).await?,
            "species",
            "species",
        )],
        Resource::Continent => vec![
            DependentKind::from_rows(continent_countries(conn, id).await?, "country", "countries"),
            DependentKind::from_rows(continent_species(conn, id).await?, "species", "species"),
        ],
        Resource::Country => vec![DependentKind::from_rows(
            country_dependents(conn, id).await?,
            "species",
            "species",
        )],
        Resource::Species => vec![],
        rank => {
            let rank = Rank::of(rank).expect("every other resource is a rank");
            let rows = sqlx::query_as::<_, DependentRow>(rank.dependents_sql)
                .bind(id)
                .fetch_all(&mut *conn)
                .await?;
            let (singular, plural) = rank.dependents_noun;
            vec![DependentKind::from_rows(rows, singular, plural)]
        }
    };
    Ok(kinds.into_iter().flatten().collect())
}

/// `409 <R>_HAS_DEPENDENTS`: the count and up to 5 ids (data-model §5).
pub fn has_dependents(res: Resource, id: &str, kinds: &[DependentKind]) -> ApiError {
    let total: i64 = kinds.iter().map(|k| k.count).sum();
    let counted: Vec<String> = kinds.iter().map(DependentKind::describe).collect();
    let named: Vec<&str> = kinds
        .iter()
        .flat_map(|k| k.ids.iter().map(String::as_str))
        .take(5)
        .collect();
    ApiError::has_dependents(
        res,
        format!(
            "Cannot delete {} '{}': {} {} on it ({}). Delete or reassign them first.",
            res.noun(),
            truncate(id, 64),
            counted.join(" and "),
            if total == 1 { "depends" } else { "depend" },
            named.join(", ")
        ),
    )
}
