//! Species create/patch validation (spec §5.6, data-model §1, research R1, R2, R10).

use bigdecimal::BigDecimal;
use serde_json::{Map, Value};
use sqlx::PgConnection;

use super::DIETS;
use crate::api::decimal::Decimal;
use crate::api::error::FieldError;
use crate::api::input::{
    self, ECHO, Field, Obj, decimal, integer, one_of, slug, slug_list, text, truncate, url,
};

/// One measure's bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bounds {
    pub min: Decimal,
    pub max: Decimal,
}

/// A validated `size`: each measure set or `NULL`; all `None` clears every column.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SizeInput {
    pub length_m: Option<Bounds>,
    pub height_m: Option<Bounds>,
    pub weight_kg: Option<Bounds>,
}

impl SizeInput {
    /// The six columns `length_min_m … weight_max_kg` (data-model §1).
    pub fn columns(&self) -> [Option<BigDecimal>; 6] {
        let pair = |b: &Option<Bounds>| match b {
            Some(b) => (Some(b.min.as_big().clone()), Some(b.max.as_big().clone())),
            None => (None, None),
        };
        let (a, b) = pair(&self.length_m);
        let (c, d) = pair(&self.height_m);
        let (e, f) = pair(&self.weight_kg);
        [a, b, c, d, e, f]
    }
}

/// Size measures (spec §5.6).
const MEASURES: [&str; 3] = ["length_m", "height_m", "weight_kg"];

fn parse_bounds(measure: &str, v: &Value) -> Result<Bounds, Vec<FieldError>> {
    let field = format!("size.{measure}");
    let Some(map) = v.as_object() else {
        return Err(vec![FieldError::new(
            &field,
            format!("{field} must be an object such as {{\"min\": 1, \"max\": 2}}, or null."),
        )]);
    };
    let mut errors = Vec::new();
    for key in map.keys().filter(|k| *k != "min" && *k != "max") {
        errors.push(FieldError::new(
            format!("{field}.{key}"),
            format!(
                "Unknown field '{}' in {field}. Allowed fields: min, max.",
                truncate(key, ECHO)
            ),
        ));
    }
    let bound = decimal(6, 0, true, None);
    let mut read = |key: &str| {
        let name = format!("{field}.{key}");
        match map.get(key) {
            None | Some(Value::Null) => {
                errors.push(FieldError::new(&name, format!("{name} is required.")));
                None
            }
            Some(v) => match bound(&name, v) {
                Ok(d) => Some(d),
                Err(msg) => {
                    errors.push(FieldError::new(&name, msg));
                    None
                }
            },
        }
    };
    let min = read("min");
    let max = read("max");
    if let (Some(min), Some(max)) = (&min, &max)
        && min > max
    {
        errors.push(FieldError::new(
            &field,
            format!("{field}.min ({min}) must not be greater than {field}.max ({max})."),
        ));
    }
    match (min, max) {
        (Some(min), Some(max)) if errors.is_empty() => Ok(Bounds { min, max }),
        _ => Err(errors),
    }
}

/// Parses `size` (spec §5.6): `null` clears all six columns; `{}` and unknown keys are
/// rejected; each measure needs `0 < min ≤ max` with at most 6 places.
pub fn parse_size(v: &Value) -> Result<SizeInput, Vec<FieldError>> {
    let map: &Map<String, Value> = match v {
        Value::Null => return Ok(SizeInput::default()),
        Value::Object(m) => m,
        _ => {
            return Err(vec![FieldError::new(
                "size",
                "size must be an object with length_m, height_m, and/or weight_kg, or null.",
            )]);
        }
    };
    let mut errors = Vec::new();
    let mut out = SizeInput::default();
    let mut present = false;
    for (key, value) in map {
        let slot = match key.as_str() {
            "length_m" => &mut out.length_m,
            "height_m" => &mut out.height_m,
            "weight_kg" => &mut out.weight_kg,
            _ => {
                present = true;
                errors.push(FieldError::new(
                    format!("size.{key}"),
                    format!(
                        "Unknown size measure '{}'. Allowed: {}.",
                        truncate(key, ECHO),
                        MEASURES.join(", ")
                    ),
                ));
                continue;
            }
        };
        if value.is_null() {
            continue;
        }
        present = true;
        match parse_bounds(key, value) {
            Ok(b) => *slot = Some(b),
            Err(es) => errors.extend(es),
        }
    }
    if !present {
        errors.push(FieldError::new(
            "size",
            "size must contain at least one of length_m, height_m, weight_kg. Send \"size\": null to remove all size data.",
        ));
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// Reads `size` from a body: `None` when invalid (details recorded).
pub fn size_field(o: &mut Obj) -> Option<Field<SizeInput>> {
    match o.take("size") {
        Field::Absent => Some(Field::Absent),
        Field::Null => Some(Field::Null),
        Field::Value(v) => match parse_size(&v) {
            Ok(s) => Some(Field::Value(s)),
            Err(errors) => {
                for e in errors {
                    o.push(e.field, e.message);
                }
                None
            }
        },
    }
}

/// A validated create body (pure checks only).
#[derive(Debug, Clone)]
pub struct SpeciesCreate {
    pub id: String,
    pub name: String,
    pub scientific_name: String,
    pub diet: &'static str,
    pub description: String,
    pub genus_id: String,
    pub period_ids: Vec<String>,
    pub continent_ids: Vec<String>,
    pub country_ids: Vec<String>,
    pub size: SizeInput,
    pub discovery_year: Option<i32>,
    pub image_url: Option<String>,
}

/// A validated patch body: `None` keeps the stored value.
#[derive(Debug, Clone, Default)]
pub struct SpeciesPatch {
    pub name: Option<String>,
    pub scientific_name: Option<String>,
    pub diet: Option<&'static str>,
    pub description: Option<String>,
    pub genus_id: Option<String>,
    pub period_ids: Option<Vec<String>>,
    pub continent_ids: Option<Vec<String>>,
    pub country_ids: Option<Vec<String>>,
    pub size: Option<SizeInput>,
    pub discovery_year: Option<Option<i32>>,
    pub image_url: Option<Option<String>>,
}

/// A nullable optional field: `null` and absent mean "empty"/`NULL`.
fn or_default<T: Default>(f: Option<Field<T>>) -> Option<T> {
    f.map(|f| match f {
        Field::Value(v) => v,
        Field::Absent | Field::Null => T::default(),
    })
}

/// A nullable optional field in a `PATCH`: absent keeps, `null` clears.
fn patch_nullable<T: Default>(f: Option<Field<T>>) -> Option<Option<T>> {
    f.map(|f| match f {
        Field::Absent => None,
        Field::Null => Some(T::default()),
        Field::Value(v) => Some(v),
    })
}

/// Pure validation of a create body; `None` when a detail was recorded.
pub fn read_create(o: &mut Obj) -> Option<SpeciesCreate> {
    let id = o.required("id", slug);
    let name = o.required("name", input::name);
    let scientific_name = o.required("scientific_name", text(128));
    let diet = o.required("diet", one_of(DIETS));
    let description = o.required("description", text(1000));
    let genus_id = o.required("genus_id", slug);
    let period_ids = o.required("period_ids", slug_list(1, 20));
    let continent_ids = or_default(o.optional("continent_ids", slug_list(0, 20)));
    let country_ids = or_default(o.optional("country_ids", slug_list(0, 50)));
    let size = or_default(size_field(o));
    let discovery_year = o.optional("discovery_year", integer(1600)).map(opt);
    let image_url = o.optional("image_url", url).map(opt);
    Some(SpeciesCreate {
        id: id?,
        name: name?,
        scientific_name: scientific_name?,
        diet: diet?,
        description: description?,
        genus_id: genus_id?,
        period_ids: period_ids?,
        continent_ids: continent_ids?,
        country_ids: country_ids?,
        size: size?,
        discovery_year: discovery_year?,
        image_url: image_url?,
    })
}

fn opt<T>(f: Field<T>) -> Option<T> {
    match f {
        Field::Value(v) => Some(v),
        Field::Absent | Field::Null => None,
    }
}

/// Pure validation of a patch body; `None` when a detail was recorded.
pub fn read_patch(o: &mut Obj) -> Option<SpeciesPatch> {
    o.forbid(
        "id",
        "id cannot be changed. Remove it from the body; create a new resource instead.",
    );
    o.require_some_field();
    let name = o.patch("name", input::name);
    let scientific_name = o.patch("scientific_name", text(128));
    let diet = o.patch("diet", one_of(DIETS));
    let description = o.patch("description", text(1000));
    let genus_id = o.patch("genus_id", slug);
    let period_ids = o.patch("period_ids", slug_list(1, 20));
    let continent_ids = patch_nullable(o.optional("continent_ids", slug_list(0, 20)));
    let country_ids = patch_nullable(o.optional("country_ids", slug_list(0, 50)));
    let size = patch_nullable(size_field(o));
    let discovery_year = o
        .optional("discovery_year", integer(1600))
        .map(|f| match f {
            Field::Absent => None,
            Field::Null => Some(None),
            Field::Value(v) => Some(Some(v)),
        });
    let image_url = o.optional("image_url", url).map(|f| match f {
        Field::Absent => None,
        Field::Null => Some(None),
        Field::Value(v) => Some(Some(v)),
    });
    Some(SpeciesPatch {
        name: name?,
        scientific_name: scientific_name?,
        diet: diet?,
        description: description?,
        genus_id: genus_id?,
        period_ids: period_ids?,
        continent_ids: continent_ids?,
        country_ids: country_ids?,
        size: size?,
        discovery_year: discovery_year?,
        image_url: image_url?,
    })
}

// ---------------------------------------------------------------- database checks (research R9)

/// References and the year to check against the database; `None` skips a check.
#[derive(Debug, Default)]
pub struct Refs<'a> {
    pub genus_id: Option<&'a str>,
    pub period_ids: Option<&'a [String]>,
    pub continent_ids: Option<&'a [String]>,
    pub country_ids: Option<&'a [String]>,
    pub discovery_year: Option<i32>,
}

/// Ids of `wanted` missing from `found`, in request order.
fn missing<'a>(wanted: &'a [String], found: &[String]) -> Vec<&'a str> {
    wanted
        .iter()
        .filter(|w| !found.contains(w))
        .map(String::as_str)
        .collect()
}

fn push_missing(o: &mut Obj, field: &str, noun: &str, missing: &[&str]) {
    if !missing.is_empty() {
        o.push(
            field,
            format!(
                "{field} lists {noun} that do not exist: {}.",
                missing.join(", ")
            ),
        );
    }
}

/// Q-W4 reference checks and the Q-W11 year bound, for fields that passed pure validation.
pub async fn check_refs(conn: &mut PgConnection, o: &mut Obj, refs: Refs<'_>) -> sqlx::Result<()> {
    if let Some(genus) = refs.genus_id {
        let ids = vec![genus.to_string()];
        let found = sqlx::query_scalar!("SELECT id FROM genera WHERE id = ANY($1::text[])", &ids)
            .fetch_all(&mut *conn)
            .await?;
        if found.is_empty() {
            o.push(
                "genus_id",
                format!(
                    "genus_id '{}' does not exist. Send the id of an existing genus.",
                    truncate(genus, ECHO)
                ),
            );
        }
    }
    if let Some(ids) = refs.period_ids {
        let found = sqlx::query_scalar!("SELECT id FROM periods WHERE id = ANY($1::text[])", ids)
            .fetch_all(&mut *conn)
            .await?;
        push_missing(o, "period_ids", "periods", &missing(ids, &found));
    }
    if let Some(ids) = refs.continent_ids.filter(|i| !i.is_empty()) {
        let found =
            sqlx::query_scalar!("SELECT id FROM continents WHERE id = ANY($1::text[])", ids)
                .fetch_all(&mut *conn)
                .await?;
        push_missing(o, "continent_ids", "continents", &missing(ids, &found));
    }
    if let Some(ids) = refs.country_ids.filter(|i| !i.is_empty()) {
        let found = sqlx::query_scalar!("SELECT id FROM countries WHERE id = ANY($1::text[])", ids)
            .fetch_all(&mut *conn)
            .await?;
        push_missing(o, "country_ids", "countries", &missing(ids, &found));
    }
    if let Some(year) = refs.discovery_year {
        let now = sqlx::query_scalar!(
            "SELECT extract(year FROM now() AT TIME ZONE 'UTC')::int AS \"year!\""
        )
        .fetch_one(&mut *conn)
        .await?;
        if year > now {
            o.push(
                "discovery_year",
                format!("discovery_year ({year}) must not be after the current year ({now}, UTC)."),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use serde_json::json;

    use super::*;

    fn d(text: &str) -> Decimal {
        Decimal::parse(&serde_json::Number::from_str(text).unwrap(), 6).unwrap()
    }

    fn fields(v: Value) -> Vec<String> {
        let mut f: Vec<String> = parse_size(&v)
            .unwrap_err()
            .into_iter()
            .map(|e| e.field)
            .collect();
        f.sort();
        f
    }

    #[test]
    fn null_clears_every_measure() {
        let s = parse_size(&Value::Null).unwrap();
        assert_eq!(s, SizeInput::default());
    }

    #[test]
    fn empty_object_and_non_objects_are_rejected() {
        assert_eq!(fields(json!({})), ["size"]);
        assert_eq!(fields(json!({ "length_m": null })), ["size"]);
        assert_eq!(fields(json!(5)), ["size"]);
        assert_eq!(fields(json!([])), ["size"]);
    }

    #[test]
    fn null_measure_is_absent() {
        let s =
            parse_size(&json!({ "length_m": { "min": 1, "max": 2 }, "height_m": null })).unwrap();
        assert_eq!(
            s,
            SizeInput {
                length_m: Some(Bounds {
                    min: d("1"),
                    max: d("2")
                }),
                height_m: None,
                weight_kg: None,
            }
        );
    }

    #[test]
    fn unknown_measure_keys_are_rejected() {
        assert_eq!(
            fields(json!({ "length_m": { "min": 1, "max": 2 }, "wingspan_m": 3 })),
            ["size.wingspan_m"]
        );
        assert_eq!(
            fields(json!({ "length_m": { "min": 1, "max": 2, "avg": 1.5 } })),
            ["size.length_m.avg"]
        );
    }

    #[test]
    fn bounds_are_required_positive_and_exact() {
        assert_eq!(
            fields(json!({ "length_m": { "max": 2 } })),
            ["size.length_m.min"]
        );
        assert_eq!(
            fields(json!({ "height_m": { "min": 2 } })),
            ["size.height_m.max"]
        );
        assert_eq!(
            fields(json!({ "height_m": {} })),
            ["size.height_m.max", "size.height_m.min"]
        );
        assert_eq!(fields(json!({ "height_m": 3 })), ["size.height_m"]);
        assert_eq!(
            fields(json!({ "length_m": { "min": 0, "max": 2 } })),
            ["size.length_m.min"]
        );
        assert_eq!(
            fields(json!({ "length_m": { "min": -1, "max": 2 } })),
            ["size.length_m.min"]
        );
        assert_eq!(
            fields(json!({ "weight_kg": { "min": 1, "max": 1.0000001 } })),
            ["size.weight_kg.max"]
        );
        assert_eq!(
            fields(json!({ "weight_kg": { "min": "1", "max": 2 } })),
            ["size.weight_kg.min"]
        );
        let edge: Value = serde_json::from_str(
            r#"{ "weight_kg": { "min": 0.000001, "max": 999999999999999.999999 } }"#,
        )
        .unwrap();
        let s = parse_size(&edge);
        assert!(s.is_ok(), "{s:?}");
    }

    #[test]
    fn min_must_not_exceed_max() {
        assert_eq!(
            fields(json!({ "length_m": { "min": 12, "max": 11 } })),
            ["size.length_m"]
        );
        let s = parse_size(&json!({ "length_m": { "min": 11, "max": 11.0 } })).unwrap();
        assert_eq!(s.length_m.unwrap().max, d("11"));
    }

    #[test]
    fn only_sent_measures_are_set() {
        let s = parse_size(&json!({ "length_m": { "min": 11.0, "max": 12.3 } })).unwrap();
        assert_eq!(
            s.length_m,
            Some(Bounds {
                min: d("11"),
                max: d("12.3")
            })
        );
        assert_eq!(s.height_m, None);
        assert_eq!(s.weight_kg, None);
    }

    #[test]
    fn every_problem_is_reported() {
        assert_eq!(
            fields(json!({
                "length_m": { "min": 0 },
                "height_m": { "min": 3, "max": 2 },
                "mass": 1,
            })),
            [
                "size.height_m",
                "size.length_m.max",
                "size.length_m.min",
                "size.mass"
            ]
        );
    }
}
