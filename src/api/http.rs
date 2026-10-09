//! Write-body reader (research R5), cached `GET` responses (research R7), envelopes.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

use actix_web::http::header;
use actix_web::{HttpRequest, HttpResponse, HttpResponseBuilder, ResponseError, web};
use serde::Serialize;
use serde_json::Value;

use super::error::ApiError;
use super::params::Pagination;

/// Largest accepted write body (spec §4.10).
pub const MAX_BODY_BYTES: usize = 65_536;

/// Time a client has to send the whole write body (FR-029, research R11).
pub const BODY_DEADLINE: Duration = Duration::from_secs(30);

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
pub fn is_json_content_type(value: Option<&str>) -> bool {
    value
        .and_then(|v| v.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

/// Strips the weak prefix `W/` of an entity tag.
fn opaque(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// Weak comparison of `If-None-Match` against `etag` (RFC 9110 §13.1.2).
pub fn if_none_match_matches(header: &str, etag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    header
        .split(',')
        .map(str::trim)
        .any(|tag| !tag.is_empty() && opaque(tag) == opaque(etag))
}

/// Reads a write body: size `413`, content type `415`, a body not complete within
/// [`BODY_DEADLINE`] `408` (plan §Deviations D2), JSON `400` (research R5).
pub async fn read_json(req: &HttpRequest, payload: web::Payload) -> Result<Value, ApiError> {
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    if declared.is_some_and(|n| n > MAX_BODY_BYTES as u64) {
        return Err(ApiError::payload_too_large());
    }
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if !is_json_content_type(content_type) {
        return Err(ApiError::unsupported_media_type());
    }
    let bytes =
        actix_web::rt::time::timeout(BODY_DEADLINE, payload.to_bytes_limited(MAX_BODY_BYTES))
            .await
            .map_err(|_| ApiError::request_timeout())?
            .map_err(|_| ApiError::payload_too_large())?
            .map_err(|_| {
                ApiError::invalid_json("The request body could not be read. Send it again.".into())
            })?;
    serde_json::from_slice(&bytes).map_err(|e| {
        ApiError::invalid_json(format!(
            "The request body is not valid JSON (line {}, column {}). \
             Check quotes, commas, and brackets.",
            e.line(),
            e.column()
        ))
    })
}

/// `Cache-Control` of successful reads (spec §4.9).
const CACHE_CONTROL: &str = "public, max-age=300";

/// Strong `ETag` over the exact response bytes (research R7).
fn etag_of(bytes: &[u8]) -> String {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("\"{:016x}\"", h.finish())
}

/// A `200` with `ETag` and `Cache-Control`, or `304` when `If-None-Match` matches.
pub fn cached_json<T: Serialize>(req: &HttpRequest, body: &T) -> HttpResponse {
    let bytes = match serde_json::to_vec(body) {
        Ok(b) => b,
        Err(_) => return ApiError::internal().error_response(),
    };
    let etag = etag_of(&bytes);
    let matches = req
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| if_none_match_matches(h, &etag));
    let mut res = if matches {
        HttpResponse::NotModified()
    } else {
        HttpResponse::Ok()
    };
    res.insert_header((header::ETAG, etag))
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL));
    if matches {
        res.finish()
    } else {
        res.content_type("application/json").body(bytes)
    }
}

fn json_response<T: Serialize>(mut res: HttpResponseBuilder, body: &T) -> HttpResponse {
    match serde_json::to_vec(body) {
        Ok(bytes) => res.content_type("application/json").body(bytes),
        Err(_) => ApiError::internal().error_response(),
    }
}

/// `201 Created` with `Location`.
pub fn created<T: Serialize>(location: String, body: &T) -> HttpResponse {
    let mut res = HttpResponse::Created();
    res.insert_header((header::LOCATION, location));
    json_response(res, body)
}

/// `200 OK` for a `PATCH`.
pub fn updated<T: Serialize>(body: &T) -> HttpResponse {
    json_response(HttpResponse::Ok(), body)
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
