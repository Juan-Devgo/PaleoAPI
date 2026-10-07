//! Species create/patch validation (spec §5.6).

use serde_json::Value;

use crate::api::decimal::Decimal;
use crate::api::error::FieldError;

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

/// Parses `size` (spec §5.6): `null` clears all six columns; `{}` and unknown keys are
/// rejected; each measure needs `0 < min ≤ max` with at most 6 places.
pub fn parse_size(_v: &Value) -> Result<SizeInput, Vec<FieldError>> {
    todo!()
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
        let s =
            parse_size(&json!({ "weight_kg": { "min": 0.000001, "max": 999999999999999.999999 } }));
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
