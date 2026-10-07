//! Query-string parsing: duplicates, `page`/`limit`, `sort`, filters, `q` (spec §4.6–§4.8).

use std::collections::HashMap;

use serde::Serialize;

use super::error::ApiError;

/// `page` and `limit` (spec §4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub page: u32,
    pub limit: u32,
}

impl Page {
    pub fn offset(&self) -> i64 {
        (i64::from(self.page) - 1) * i64::from(self.limit)
    }
}

/// A sort key and direction (spec §4.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort<S> {
    pub key: S,
    pub desc: bool,
}

/// A parsed list request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQuery<F, S> {
    pub page: Page,
    pub sort: Sort<S>,
    pub filter: F,
}

/// The `pagination` object of a list envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Pagination {
    pub page: u32,
    pub limit: u32,
    pub total_items: i64,
    pub total_pages: i64,
}

impl Pagination {
    pub fn new(page: Page, total_items: i64) -> Self {
        let limit = i64::from(page.limit);
        Self {
            page: page.page,
            limit: page.limit,
            total_items,
            total_pages: (total_items + limit - 1) / limit,
        }
    }
}

/// The known parameters of one request, each present at most once.
#[derive(Debug, Clone, Default)]
pub struct Params {
    values: HashMap<&'static str, String>,
}

impl Params {
    /// Parses `query`, keeping only `known` keys; a repeated known key → `400`.
    pub fn parse(_query: &str, _known: &[&'static str]) -> Result<Self, ApiError> {
        todo!()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// `page` and `limit`, with defaults `1` and `20`.
    pub fn page(&self) -> Result<Page, ApiError> {
        todo!()
    }

    /// `sort=<field>` or `sort=-<field>` from `allowed`.
    pub fn sort<S: Copy>(
        &self,
        _allowed: &[(&'static str, S)],
        _default: Sort<S>,
    ) -> Result<Sort<S>, ApiError> {
        todo!()
    }

    /// A slug filter.
    pub fn slug(&self, _key: &'static str) -> Result<Option<String>, ApiError> {
        todo!()
    }

    /// An enum filter.
    pub fn one_of(
        &self,
        _key: &'static str,
        _allowed: &'static [&'static str],
    ) -> Result<Option<&'static str>, ApiError> {
        todo!()
    }

    /// `q`: trimmed, 3–64 characters, returned as an escaped `ILIKE` pattern `%…%`.
    pub fn q(&self) -> Result<Option<String>, ApiError> {
        todo!()
    }
}

/// Escapes `\`, `%`, `_` for `LIKE … ESCAPE '\'`.
pub fn escape_like(_s: &str) -> String {
    todo!()
}
