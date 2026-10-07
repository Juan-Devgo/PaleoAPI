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

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: &[&str] = &["page", "limit", "sort", "diet", "era", "q"];
    const DIETS: &[&str] = &["carnivore", "herbivore"];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Key {
        Name,
        Year,
    }
    const SORTS: &[(&str, Key)] = &[("name", Key::Name), ("discovery_year", Key::Year)];
    const DEFAULT: Sort<Key> = Sort {
        key: Key::Name,
        desc: false,
    };

    fn params(q: &str) -> Params {
        Params::parse(q, KNOWN).unwrap()
    }

    #[track_caller]
    fn bad_request<T: std::fmt::Debug>(r: Result<T, ApiError>) -> String {
        let e = r.unwrap_err();
        assert_eq!(e.status().as_u16(), 400);
        assert_eq!(e.code(), "INVALID_QUERY_PARAMETER");
        e.message().to_string()
    }

    #[test]
    fn page_defaults_and_ranges() {
        assert_eq!(params("").page().unwrap(), Page { page: 1, limit: 20 });
        assert_eq!(
            params("page=1000000&limit=100").page().unwrap(),
            Page {
                page: 1_000_000,
                limit: 100
            }
        );
        assert_eq!(params("page=3&limit=7").page().unwrap().offset(), 14);
        for q in [
            "limit=101",
            "limit=0",
            "page=0",
            "page=1000001",
            "page=abc",
            "page=",
            "limit=",
            "page=+5",
            "page=05x",
            "page=-1",
            "page=1.0",
            "limit=99999999999999999999",
        ] {
            bad_request(params(q).page());
        }
    }

    #[test]
    fn repeated_known_keys_are_rejected_and_unknown_keys_ignored() {
        bad_request(Params::parse("page=1&page=2", KNOWN));
        bad_request(Params::parse("diet=carnivore&diet=carnivore", KNOWN));
        let p = params("foo=1&foo=2&page=2");
        assert_eq!(p.get("foo"), None);
        assert_eq!(p.page().unwrap().page, 2);
    }

    #[test]
    fn sort_allow_list() {
        assert_eq!(params("").sort(SORTS, DEFAULT).unwrap(), DEFAULT);
        assert_eq!(
            params("sort=-discovery_year").sort(SORTS, DEFAULT).unwrap(),
            Sort {
                key: Key::Year,
                desc: true
            }
        );
        assert_eq!(
            params("sort=name").sort(SORTS, DEFAULT).unwrap(),
            Sort {
                key: Key::Name,
                desc: false
            }
        );
        let msg = bad_request(params("sort=size").sort(SORTS, DEFAULT));
        assert!(
            msg.contains("name, -name, discovery_year, -discovery_year"),
            "{msg}"
        );
        for q in ["sort=", "sort=--name", "sort=Name", "sort=+name"] {
            bad_request(params(q).sort(SORTS, DEFAULT));
        }
    }

    #[test]
    fn filters() {
        assert_eq!(params("").slug("era").unwrap(), None);
        assert_eq!(
            params("era=mesozoic").slug("era").unwrap().as_deref(),
            Some("mesozoic")
        );
        bad_request(params("era=").slug("era"));
        bad_request(params("era=Not_A_Slug").slug("era"));
        assert_eq!(
            params("diet=herbivore").one_of("diet", DIETS).unwrap(),
            Some("herbivore")
        );
        assert_eq!(params("").one_of("diet", DIETS).unwrap(), None);
        bad_request(params("diet=banana").one_of("diet", DIETS));
        bad_request(params("diet=").one_of("diet", DIETS));
    }

    #[test]
    fn q_bounds_and_escaping() {
        assert_eq!(params("").q().unwrap(), None);
        assert_eq!(params("q=REX").q().unwrap().as_deref(), Some("%REX%"));
        assert_eq!(params("q=%20%20rex%20").q().unwrap().as_deref(), Some("%rex%"));
        assert_eq!(
            params("q=%25%25_").q().unwrap().as_deref(),
            Some(r"%\%\%\_%")
        );
        assert_eq!(params("q=a%5Cb").q().unwrap().as_deref(), Some(r"%a\\b%"));
        bad_request(params("q=re").q());
        bad_request(params("q=%20re%20").q());
        bad_request(params("q=").q());
        let max = "ñ".repeat(64);
        assert!(params(&format!("q={max}")).q().is_ok());
        bad_request(params(&format!("q={max}x")).q());
    }

    #[test]
    fn escape_like_escapes_backslash_percent_underscore() {
        assert_eq!(escape_like(r"a\%_b"), r"a\\\%\_b");
    }

    #[test]
    fn pagination_totals() {
        let p = Page { page: 1, limit: 20 };
        assert_eq!(Pagination::new(p, 0).total_pages, 0);
        assert_eq!(Pagination::new(p, 1).total_pages, 1);
        assert_eq!(Pagination::new(p, 20).total_pages, 1);
        assert_eq!(Pagination::new(p, 342).total_pages, 18);
    }
}
