//! Database errors → responses (data-model §4, §5).

/// Whether `constraint` (a constraint, unique index, or trigger-raised name) has a row in the error map.
pub fn is_mapped(_constraint: &str) -> bool {
    todo!()
}
