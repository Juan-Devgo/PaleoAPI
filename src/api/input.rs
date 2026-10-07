//! Write-body walking and shared field validators (spec §4.2, §4.10, §4.13; research R2, R10).

use serde_json::{Map, Value};

use super::decimal::Decimal;
use super::error::{ApiError, FieldError};

/// A body field: omitted, sent as `null`, or sent with a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field<T> {
    Absent,
    Null,
    Value(T),
}

/// A field parser: `(field name, value) → typed value or message`.
pub trait Parse<T>: FnOnce(&str, &Value) -> Result<T, String> {}
impl<T, F: FnOnce(&str, &Value) -> Result<T, String>> Parse<T> for F {}

/// Walks one JSON object and collects every problem.
#[derive(Debug)]
pub struct Obj {
    map: Map<String, Value>,
    known: Vec<&'static str>,
    errors: Vec<FieldError>,
    was_empty: bool,
}

impl Obj {
    /// A body that is not an object is one detail on field `""`.
    pub fn new(_body: Value) -> Result<Self, ApiError> {
        todo!()
    }

    /// Reads a field without validating it.
    pub fn take(&mut self, _key: &'static str) -> Field<Value> {
        todo!()
    }

    /// Required on create: absent, `null`, or invalid → a detail and `None`.
    pub fn required<T>(&mut self, _key: &'static str, _parse: impl Parse<T>) -> Option<T> {
        todo!()
    }

    /// Optional field: `None` when invalid (a detail was recorded).
    pub fn optional<T>(&mut self, _key: &'static str, _parse: impl Parse<T>) -> Option<Field<T>> {
        todo!()
    }

    /// A required field in a `PATCH`: absent → `Some(None)`; `null` or invalid → `None`.
    pub fn patch<T>(&mut self, _key: &'static str, _parse: impl Parse<T>) -> Option<Option<T>> {
        todo!()
    }

    /// Rejects a field that must not be sent (for example `id` in a `PATCH`).
    pub fn forbid(&mut self, _key: &'static str, _message: &str) {
        todo!()
    }

    /// Rejects an empty `PATCH` body.
    pub fn require_some_field(&mut self) {
        todo!()
    }

    /// Records a problem found outside the walker (domain and database checks).
    pub fn push(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors.push(FieldError::new(field, message));
    }

    /// Whether a problem was recorded for `field`.
    pub fn has_error(&self, field: &str) -> bool {
        self.errors.iter().any(|e| e.field == field)
    }

    /// Unknown fields become details; any detail → `422 VALIDATION_FAILED`.
    pub fn finish(self) -> Result<(), ApiError> {
        todo!()
    }
}

/// Slug of spec §4.2: 2–64 of `a–z`, `0–9`, single inner hyphens.
pub fn is_slug(_s: &str) -> bool {
    todo!()
}

/// Truncates to `max` characters, adding `…` when cut.
pub fn truncate(_s: &str, _max: usize) -> String {
    todo!()
}

/// A slug value.
pub fn slug(_field: &str, _v: &Value) -> Result<String, String> {
    todo!()
}

/// A `name`: trimmed, 1–64 characters.
pub fn name(field: &str, v: &Value) -> Result<String, String> {
    text(64)(field, v)
}

/// Trimmed text of 1–`max` characters.
pub fn text(_max: usize) -> impl Fn(&str, &Value) -> Result<String, String> {
    |_, _| todo!()
}

/// One of a fixed set of lowercase strings.
pub fn one_of(
    _allowed: &'static [&'static str],
) -> impl Fn(&str, &Value) -> Result<&'static str, String> {
    |_, _| todo!()
}

/// A decimal with at most `places` decimal places in `[min, max]` (`min` excluded when `min_exclusive`).
pub fn decimal(
    _places: u32,
    _min: i64,
    _min_exclusive: bool,
    _max: Option<i64>,
) -> impl Fn(&str, &Value) -> Result<Decimal, String> {
    |_, _| todo!()
}

/// A list of distinct slugs with `min..=max` items.
pub fn slug_list(_min: usize, _max: usize) -> impl Fn(&str, &Value) -> Result<Vec<String>, String> {
    |_, _| todo!()
}

/// A JSON integer `≥ min` (no fraction, no exponent).
pub fn integer(_min: i32) -> impl Fn(&str, &Value) -> Result<i32, String> {
    |_, _| todo!()
}

/// An absolute `http`/`https` URL of at most 2048 characters; the scheme is lowercased.
pub fn url(_field: &str, _v: &Value) -> Result<String, String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn obj(v: Value) -> Obj {
        Obj::new(v).unwrap()
    }

    /// The detail fields of a finished walker (sorted).
    fn fields(o: Obj) -> Vec<String> {
        match o.finish() {
            Ok(()) => vec![],
            Err(e) => {
                assert_eq!(e.status().as_u16(), 422);
                assert_eq!(e.code(), "VALIDATION_FAILED");
                let mut f: Vec<String> = e.details().iter().map(|d| d.field.clone()).collect();
                f.sort();
                f
            }
        }
    }

    #[test]
    fn slug_cases() {
        let too_long = format!("a{}", "-b".repeat(32));
        assert_eq!(too_long.len(), 65);
        for bad in [
            "", "A", "a", "-ab", "ab-", "a--b", "Ab", "a b", "a_b", "ñu", &too_long,
        ] {
            assert!(!is_slug(bad), "{bad:?}");
            assert!(slug("id", &json!(bad)).is_err(), "{bad:?}");
        }
        for good in ["a1-b2", "ab", "tyrannosaurus-rex", &"a".repeat(64)] {
            assert!(is_slug(good), "{good:?}");
            assert_eq!(slug("id", &json!(good)).unwrap(), good);
        }
        assert!(slug("id", &json!(12)).is_err());
    }

    #[test]
    fn text_is_trimmed_and_counted_in_characters() {
        let max = "ñ".repeat(64);
        assert_eq!(name("name", &json!(max)).unwrap(), max);
        assert!(name("name", &json!("ñ".repeat(65))).is_err());
        assert_eq!(name("name", &json!("  Mesozoic \t")).unwrap(), "Mesozoic");
        assert_eq!(name("name", &json!("\u{a0}Rex\u{a0}")).unwrap(), "Rex");
        for blank in ["", "   ", "\u{a0}", "\n\t"] {
            assert!(name("name", &json!(blank)).is_err(), "{blank:?}");
        }
        assert!(name("name", &json!(5)).is_err());
        assert_eq!(text(1000)("description", &json!(" x ")).unwrap(), "x");
    }

    #[test]
    fn field_absent_null_value() {
        let mut o = obj(json!({ "a": null, "b": "x" }));
        assert_eq!(o.take("a"), Field::Null);
        assert_eq!(o.take("b"), Field::Value(json!("x")));
        assert_eq!(o.take("c"), Field::Absent);
        assert!(fields(o).is_empty());

        let mut o = obj(json!({ "a": null, "b": "x" }));
        assert_eq!(o.optional("a", name), Some(Field::Null));
        assert_eq!(o.optional("b", name), Some(Field::Value("x".to_string())));
        assert_eq!(o.optional("c", name), Some(Field::Absent));
        assert_eq!(o.optional("d", name), Some(Field::Absent));
        assert!(fields(o).is_empty());
    }

    #[test]
    fn required_fields_on_create() {
        let mut o = obj(json!({ "name": null, "id": "Bad" }));
        assert_eq!(o.required("name", name), None);
        assert_eq!(o.required("id", slug), None);
        assert_eq!(o.required("start_mya", decimal(3, 0, false, Some(4600))), None);
        assert_eq!(fields(o), ["id", "name", "start_mya"]);
    }

    #[test]
    fn patch_fields() {
        let mut o = obj(json!({ "name": null, "start_mya": 10 }));
        assert_eq!(o.patch("name", self::name), None);
        assert_eq!(o.patch("end_mya", decimal(3, 0, false, Some(4600))), Some(None));
        assert!(o.patch("start_mya", decimal(3, 0, false, Some(4600))).is_some());
        assert_eq!(fields(o), ["name"]);
    }

    #[test]
    fn unknown_and_response_only_fields_are_reported() {
        let mut o = obj(json!({ "name": "Ok", "nmae": "typo", "era": { "id": "x" } }));
        o.required("name", name);
        assert_eq!(fields(o), ["era", "nmae"]);
    }

    #[test]
    fn id_in_patch_and_empty_patch() {
        let mut o = obj(json!({ "id": "new-id" }));
        o.forbid("id", "id cannot be changed.");
        o.require_some_field();
        assert_eq!(fields(o), ["id"]);

        let mut o = obj(json!({}));
        o.require_some_field();
        assert_eq!(fields(o), [""]);
    }

    #[test]
    fn non_object_body_is_one_detail_on_the_whole_body() {
        for body in [json!([]), json!("x"), json!(1), json!(null)] {
            let err = Obj::new(body).unwrap_err();
            assert_eq!(err.status().as_u16(), 422);
            assert_eq!(err.details().len(), 1);
            assert_eq!(err.details()[0].field, "");
        }
    }

    #[test]
    fn every_problem_is_reported() {
        let mut o = obj(json!({ "id": "A", "name": "", "start_mya": 1.0001, "x": 1 }));
        o.required("id", slug);
        o.required("name", name);
        o.required("start_mya", decimal(3, 0, false, Some(4600)));
        o.required("end_mya", decimal(3, 0, false, Some(4600)));
        assert_eq!(fields(o), ["end_mya", "id", "name", "start_mya", "x"]);
    }

    #[test]
    fn decimal_bounds_and_places() {
        let mya = decimal(3, 0, false, Some(4600));
        assert!(mya("start_mya", &json!(4600)).is_ok());
        assert!(mya("start_mya", &json!(0)).is_ok());
        assert!(mya("start_mya", &json!(4600.001)).is_err());
        assert!(mya("start_mya", &json!(-0.001)).is_err());
        assert!(mya("start_mya", &json!(1.0001)).is_err());
        assert!(mya("start_mya", &json!("1")).is_err());
        let bound = decimal(6, 0, true, None);
        assert!(bound("size.length_m.min", &json!(0)).is_err());
        assert!(bound("size.length_m.min", &json!(0.000001)).is_ok());
        let huge: Value = serde_json::from_str("1e15").unwrap();
        assert!(bound("size.length_m.min", &huge).is_err());
    }

    #[test]
    fn slug_lists() {
        let list = slug_list(1, 3);
        assert_eq!(
            list("period_ids", &json!(["a1", "b2"])).unwrap(),
            ["a1", "b2"]
        );
        assert!(list("period_ids", &json!([])).is_err());
        assert!(list("period_ids", &json!(["a1", "a1"])).is_err());
        assert!(list("period_ids", &json!(["a1", "b2", "c3", "d4"])).is_err());
        assert!(list("period_ids", &json!(["Bad"])).is_err());
        assert!(list("period_ids", &json!("a1")).is_err());
        assert!(slug_list(0, 3)("continent_ids", &json!([])).unwrap().is_empty());
    }

    #[test]
    fn urls() {
        assert_eq!(url("image_url", &json!("HTTP://x")).unwrap(), "http://x");
        assert_eq!(
            url("image_url", &json!("HttpS://example.org/a.png")).unwrap(),
            "https://example.org/a.png"
        );
        assert_eq!(url("image_url", &json!(" https://x/y ")).unwrap(), "https://x/y");
        for bad in [
            "ftp://x",
            "http:///x",
            "https://?q",
            "https://#f",
            "http://",
            "x",
            "http://a b",
            "http://a\u{7}b",
        ] {
            assert!(url("image_url", &json!(bad)).is_err(), "{bad:?}");
        }
        let max = format!("https://x/{}", "a".repeat(2048 - 10));
        assert_eq!(max.chars().count(), 2048);
        assert!(url("image_url", &json!(max)).is_ok());
        assert!(url("image_url", &json!(format!("{max}a"))).is_err());
    }

    #[test]
    fn integer_years() {
        let year = integer(1600);
        assert_eq!(year("discovery_year", &json!(1905)).unwrap(), 1905);
        let fractional: Value = serde_json::from_str("1905.0").unwrap();
        assert!(year("discovery_year", &fractional).is_err());
        assert!(year("discovery_year", &json!(1599)).is_err());
        assert!(year("discovery_year", &json!("1905")).is_err());
        let huge: Value = serde_json::from_str("99999999999").unwrap();
        assert!(year("discovery_year", &huge).is_err());
    }

    #[test]
    fn enums() {
        let diet = one_of(&["carnivore", "herbivore"]);
        assert_eq!(diet("diet", &json!("carnivore")).unwrap(), "carnivore");
        assert!(diet("diet", &json!("Carnivore")).is_err());
        assert!(diet("diet", &json!("banana")).is_err());
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate(&"ñ".repeat(40), 32), format!("{}…", "ñ".repeat(32)));
    }
}
