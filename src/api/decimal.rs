//! Exact decimals from JSON number text to `numeric` and back (spec §4.13, research R1).

use std::fmt;

use bigdecimal::BigDecimal;
use serde::{Serialize, Serializer};

/// Why a JSON number is not an acceptable decimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecimalError {
    /// Text longer than 40 characters, or `|v| ≥ 10^15`.
    TooLarge,
    /// More significant decimal places than allowed.
    TooManyPlaces { max: u32 },
}

/// A normalized decimal (no trailing zeros), serialized as a plain JSON number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Decimal(BigDecimal);

impl Decimal {
    /// Parses a JSON number with at most `max_places` significant decimal places.
    pub fn parse(_n: &serde_json::Number, _max_places: u32) -> Result<Self, DecimalError> {
        todo!()
    }

    /// A stored value, normalized.
    pub fn from_db(_v: BigDecimal) -> Self {
        todo!()
    }

    pub fn as_big(&self) -> &BigDecimal {
        &self.0
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, _s: S) -> Result<S::Ok, S::Error> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn num(text: &str) -> serde_json::Number {
        serde_json::Number::from_str(text).unwrap()
    }

    fn json(d: &Decimal) -> String {
        serde_json::to_string(d).unwrap()
    }

    #[test]
    fn accepts_up_to_the_allowed_places() {
        let d = Decimal::parse(&num("251.902"), 3).unwrap();
        assert_eq!(json(&d), "251.902");
        assert_eq!(d.to_string(), "251.902");
    }

    #[test]
    fn trailing_zeros_are_not_places_and_are_dropped() {
        let d = Decimal::parse(&num("66.0000"), 2).unwrap();
        assert_eq!(json(&d), "66");
        assert_eq!(d, Decimal::parse(&num("66"), 0).unwrap());
    }

    #[test]
    fn rejects_extra_places() {
        assert_eq!(
            Decimal::parse(&num("66.0001"), 3),
            Err(DecimalError::TooManyPlaces { max: 3 })
        );
        assert_eq!(
            Decimal::parse(&num("0.0000001"), 6),
            Err(DecimalError::TooManyPlaces { max: 6 })
        );
    }

    #[test]
    fn exponents_are_exact() {
        assert_eq!(json(&Decimal::parse(&num("1e3"), 0).unwrap()), "1000");
        assert_eq!(json(&Decimal::parse(&num("1.5E-2"), 3).unwrap()), "0.015");
    }

    #[test]
    fn rejects_long_text_and_huge_values() {
        let long = format!("1.{}", "0".repeat(39));
        assert_eq!(long.len(), 41);
        assert_eq!(Decimal::parse(&num(&long), 6), Err(DecimalError::TooLarge));
        for text in ["1e15", "1000000000000000", "-1e15", "1e1000000000", "12345678901234567"] {
            assert_eq!(
                Decimal::parse(&num(text), 6),
                Err(DecimalError::TooLarge),
                "{text}"
            );
        }
        let below = Decimal::parse(&num("999999999999999.999999"), 6).unwrap();
        assert_eq!(json(&below), "999999999999999.999999");
    }

    #[test]
    fn zero_with_any_exponent_is_zero() {
        assert_eq!(json(&Decimal::parse(&num("0e1000"), 0).unwrap()), "0");
        assert_eq!(json(&Decimal::parse(&num("0.000"), 0).unwrap()), "0");
    }

    #[test]
    fn output_never_uses_an_exponent() {
        let d = Decimal::parse(&num("0.000001"), 6).unwrap();
        assert_eq!(json(&d), "0.000001");
        let d = Decimal::from_db(BigDecimal::from_str("1E+2").unwrap());
        assert_eq!(json(&d), "100");
        let d = Decimal::from_db(BigDecimal::from_str("4600.000").unwrap());
        assert_eq!(json(&d), "4600");
    }
}
