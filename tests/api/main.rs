//! HTTP contract tests (spec 002 §6). Every `#[sqlx::test]` drives
//! `paleo_api::api::app` in-process against its own throwaway database (research R11).
//!
//! Run them only with the documented test command (AGENTS.md §3).

mod support;
mod tokens;

mod error_map;
mod geography;
mod geologic_time;
mod global;
mod openapi;
mod performance;
mod query_plans;
mod species;
mod species_filters;
mod taxonomy;
