//! Database integration tests. Every `#[sqlx::test]` runs in its own
//! throwaway database with all migrations applied (FR-013, FR-014).
//!
//! Run them only with the documented test command (README, Getting started).

mod support;

mod accounts;
mod collation;
mod concurrency;
mod geography;
mod geologic_time;
mod identifiers;
mod indexes;
mod isolation;
mod migrations;
mod schema_catalog;
mod species;
mod startup_checks;
mod taxonomy;
