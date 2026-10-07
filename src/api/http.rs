//! Write-body reader (research R5), cached `GET` responses (research R7), envelopes.

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Serialize;
use serde_json::Value;

use super::error::ApiError;
use super::params::Pagination;

/// Largest accepted write body (spec §4.10).
pub const MAX_BODY_BYTES: usize = 65_536;

/// `{ "data": … }`.
#[derive(Debug, Serialize)]
pub struct One<T> {
    pub data: T,
}

/// `{ "data": [ … ], "pagination": … }`.
#[derive(Debug, Serialize)]
pub struct Many<T> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}

/// Whether a `Content-Type` value is `application/json` (parameters allowed).
pub fn is_json_content_type(_value: Option<&str>) -> bool {
    todo!()
}

/// Weak comparison of `If-None-Match` against `etag` (RFC 9110 §13.1.2).
pub fn if_none_match_matches(_header: &str, _etag: &str) -> bool {
    todo!()
}

/// Reads a write body: size `413`, content type `415`, JSON `400`.
pub async fn read_json(_req: &HttpRequest, _payload: web::Payload) -> Result<Value, ApiError> {
    todo!()
}

/// A `200` with `ETag` and `Cache-Control`, or `304` when `If-None-Match` matches.
pub fn cached_json<T: Serialize>(_req: &HttpRequest, _body: &T) -> HttpResponse {
    todo!()
}

/// `201 Created` with `Location`.
pub fn created<T: Serialize>(_location: String, _body: &T) -> HttpResponse {
    todo!()
}

/// `200 OK` for a `PATCH`.
pub fn updated<T: Serialize>(_body: &T) -> HttpResponse {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_content_types() {
        for ok in [
            "application/json",
            "application/json; charset=utf-8",
            "Application/JSON",
            "application/json;charset=UTF-8",
        ] {
            assert!(is_json_content_type(Some(ok)), "{ok}");
        }
        for bad in [
            None,
            Some(""),
            Some("text/plain"),
            Some("application/jsonx"),
            Some("application/x-www-form-urlencoded"),
            Some("multipart/form-data"),
        ] {
            assert!(!is_json_content_type(bad), "{bad:?}");
        }
    }

    #[test]
    fn if_none_match_weak_comparison() {
        let tag = "\"9f2c4a1be07d3c55\"";
        assert!(if_none_match_matches(tag, tag));
        assert!(if_none_match_matches("*", tag));
        assert!(if_none_match_matches("W/\"9f2c4a1be07d3c55\"", tag));
        assert!(if_none_match_matches("\"aaa\", \"9f2c4a1be07d3c55\"", tag));
        assert!(if_none_match_matches("\"aaa\",W/\"9f2c4a1be07d3c55\"", tag));
        assert!(!if_none_match_matches("\"aaa\"", tag));
        assert!(!if_none_match_matches("9f2c4a1be07d3c55", tag));
        assert!(!if_none_match_matches("", tag));
    }
}
