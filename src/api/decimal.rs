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
