//! Query-string parsing: duplicates, `page`/`limit`, `sort`, filters, `q` (spec §4.6–§4.8).

use std::collections::HashMap;

use actix_web::web;
use serde::Serialize;

use super::error::ApiError;
use super::input::{is_slug, truncate};

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

/// Longest echoed query value.
const ECHO: usize = 32;

fn echo(v: &str) -> String {
    truncate(v, ECHO)
}

impl Params {
    /// Parses `query`, keeping only `known` keys; a repeated known key → `400`.
    pub fn parse(query: &str, known: &[&'static str]) -> Result<Self, ApiError> {
        let pairs = web::Query::<Vec<(String, String)>>::from_query(query)
            .map_err(|_| {
                ApiError::invalid_query(
                    "The query string is malformed. Use key=value pairs separated by '&'.".into(),
                )
            })?
            .into_inner();
        let mut values = HashMap::new();
        for (key, value) in pairs {
            let Some(&k) = known.iter().find(|k| **k == key) else {
                continue;
            };
            if values.insert(k, value).is_some() {
                return Err(ApiError::invalid_query(format!(
                    "Query parameter '{k}' is repeated. Send it at most once."
                )));
            }
        }
        Ok(Self { values })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn uint(&self, key: &str, default: u32, max: u32) -> Result<u32, ApiError> {
        let Some(v) = self.get(key) else {
            return Ok(default);
        };
        let err = || {
            ApiError::invalid_query(format!(
                "{key} must be an integer from 1 to {max} (got '{}').",
                echo(v)
            ))
        };
        if v.is_empty() || v.len() > 10 || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err());
        }
        match v.parse::<u32>() {
            Ok(n) if (1..=max).contains(&n) => Ok(n),
            _ => Err(err()),
        }
    }

    /// `page` and `limit`, with defaults `1` and `20`.
    pub fn page(&self) -> Result<Page, ApiError> {
        Ok(Page {
            page: self.uint("page", 1, 1_000_000)?,
            limit: self.uint("limit", 20, 100)?,
        })
    }

    /// `sort=<field>` or `sort=-<field>` from `allowed`.
    pub fn sort<S: Copy>(
        &self,
        allowed: &[(&'static str, S)],
        default: Sort<S>,
    ) -> Result<Sort<S>, ApiError> {
        let Some(v) = self.get("sort") else {
            return Ok(default);
        };
        let (field, desc) = match v.strip_prefix('-') {
            Some(rest) => (rest, true),
            None => (v, false),
        };
        match allowed.iter().find(|(name, _)| *name == field) {
            Some(&(_, key)) => Ok(Sort { key, desc }),
            None => {
                let list: Vec<String> = allowed
                    .iter()
                    .flat_map(|(name, _)| [name.to_string(), format!("-{name}")])
                    .collect();
                Err(ApiError::invalid_query(format!(
                    "sort '{}' is not supported. Use one of: {}.",
                    echo(v),
                    list.join(", ")
                )))
            }
        }
    }

    /// A slug filter.
    pub fn slug(&self, key: &'static str) -> Result<Option<String>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) if is_slug(v) => Ok(Some(v.to_string())),
            Some(v) => Err(ApiError::invalid_query(format!(
                "{key} must be an id: 2–64 lowercase letters, digits, or single hyphens (got '{}').",
                echo(v)
            ))),
        }
    }

    /// An enum filter.
    pub fn one_of(
        &self,
        key: &'static str,
        allowed: &'static [&'static str],
    ) -> Result<Option<&'static str>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => match allowed.iter().find(|a| **a == v) {
                Some(a) => Ok(Some(a)),
                None => Err(ApiError::invalid_query(format!(
                    "{key} must be one of: {} (got '{}').",
                    allowed.join(", "),
                    echo(v)
                ))),
            },
        }
    }

    /// `q`: trimmed, 3–64 characters, returned as an escaped `ILIKE` pattern `%…%`.
    pub fn q(&self) -> Result<Option<String>, ApiError> {
        let Some(v) = self.get("q") else {
            return Ok(None);
        };
        let term = v.trim();
        let n = term.chars().count();
        if !(3..=64).contains(&n) {
            return Err(ApiError::invalid_query(format!(
                "q must be 3–64 characters after trimming spaces (got {n})."
            )));
        }
        Ok(Some(format!("%{}%", escape_like(term))))
    }
}

/// Escapes `\`, `%`, `_` for `LIKE … ESCAPE '\'`.
pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
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
