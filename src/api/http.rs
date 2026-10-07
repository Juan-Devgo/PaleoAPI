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
