//! Error envelope, codes (spec §4.12), and field details (spec §4.5).

use std::borrow::Cow;
use std::fmt;

use actix_web::dev::ServiceResponse;
use actix_web::http::header::{self, HeaderValue};
use actix_web::http::{Method, StatusCode};
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;

use super::input::truncate;

/// One problem in a write body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// The resources of spec §5, for codes (`ERA_NOT_FOUND`) and messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Era,
    Period,
    Domain,
    Kingdom,
    Phylum,
    Class,
    Order,
    Family,
    Genus,
    Continent,
    Country,
    Species,
}

impl Resource {
    /// Code stem (data-model §4).
    pub fn code(self) -> &'static str {
        match self {
            Self::Era => "ERA",
            Self::Period => "PERIOD",
            Self::Domain => "DOMAIN",
            Self::Kingdom => "KINGDOM",
            Self::Phylum => "PHYLUM",
            Self::Class => "CLASS",
            Self::Order => "ORDER",
            Self::Family => "FAMILY",
            Self::Genus => "GENUS",
            Self::Continent => "CONTINENT",
            Self::Country => "COUNTRY",
            Self::Species => "SPECIES",
        }
    }

    /// Singular noun for messages.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Era => "era",
            Self::Period => "period",
            Self::Domain => "domain",
            Self::Kingdom => "kingdom",
            Self::Phylum => "phylum",
            Self::Class => "class",
            Self::Order => "order",
            Self::Family => "family",
            Self::Genus => "genus",
            Self::Continent => "continent",
            Self::Country => "country",
            Self::Species => "species",
        }
    }

    /// Plural noun for messages.
    pub fn plural(self) -> &'static str {
        match self {
            Self::Era => "eras",
            Self::Period => "periods",
            Self::Domain => "domains",
            Self::Kingdom => "kingdoms",
            Self::Phylum => "phyla",
            Self::Class => "classes",
            Self::Order => "orders",
            Self::Family => "families",
            Self::Genus => "genera",
            Self::Continent => "continents",
            Self::Country => "countries",
            Self::Species => "species",
        }
    }

    /// "a" or "an" before [`Resource::noun`].
    pub fn article(self) -> &'static str {
        match self {
            Self::Era | Self::Order => "an",
            _ => "a",
        }
    }
}

/// An error response: status, code, message, and `422` details.
#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: Cow<'static, str>,
    message: String,
    details: Vec<FieldError>,
}

/// Longest path id echoed in a message (plan §Write path).
const ID_ECHO: usize = 64;

impl ApiError {
    fn new(status: StatusCode, code: impl Into<Cow<'static, str>>, message: String) -> Self {
        Self {
            status,
            code: code.into(),
            message,
            details: Vec::new(),
        }
    }

    pub fn invalid_json(message: String) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "INVALID_JSON", message)
    }

    pub fn invalid_query(message: String) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "INVALID_QUERY_PARAMETER", message)
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "This operation requires admin credentials. \
             Send a valid admin token in the Authorization header."
                .into(),
        )
    }

    pub fn forbidden() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "This operation requires the admin role.".into(),
        )
    }

    /// `404 <R>_NOT_FOUND` naming the id.
    pub fn not_found(res: Resource, id: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            format!("{}_NOT_FOUND", res.code()),
            format!(
                "No {} found with id '{}'.",
                res.noun(),
                truncate(id, ID_ECHO)
            ),
        )
    }

    pub fn route_not_found(path: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "ROUTE_NOT_FOUND",
            format!(
                "No route matches '{}'. See the API description for available paths.",
                truncate(path, 256)
            ),
        )
    }

    /// `405` naming the method and the allowed ones.
    pub fn method_not_allowed(method: &Method, allow: &str) -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "METHOD_NOT_ALLOWED",
            format!(
                "{} is not supported on this path. Use one of: {allow}.",
                truncate(method.as_str(), 16)
            ),
        )
    }

    pub fn already_exists(res: Resource, message: String) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            format!("{}_ALREADY_EXISTS", res.code()),
            message,
        )
    }

    pub fn has_dependents(res: Resource, message: String) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            format!("{}_HAS_DEPENDENTS", res.code()),
            message,
        )
    }

    pub fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PAYLOAD_TOO_LARGE",
            "The request body is larger than 64 KB.".into(),
        )
    }

    pub fn unsupported_media_type() -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
            "Send the body as JSON with the header Content-Type: application/json.".into(),
        )
    }

    /// `422 VALIDATION_FAILED` with every detail.
    pub fn validation(details: Vec<FieldError>) -> Self {
        let n = details.len();
        let message = if n == 1 {
            "The request body has 1 invalid field. See details.".to_string()
        } else {
            format!("The request body has {n} invalid fields. See details.")
        };
        Self {
            details,
            ..Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_FAILED",
                message,
            )
        }
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Something went wrong on our side. Try again later.".into(),
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn details(&self) -> &[FieldError] {
        &self.details
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.message)
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    error: Body<'a>,
}

#[derive(Serialize)]
struct Body<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a [FieldError]>,
}

impl ApiError {
    /// The JSON envelope of spec §4.5.
    pub fn body(&self) -> Vec<u8> {
        let details =
            (self.status == StatusCode::UNPROCESSABLE_ENTITY).then_some(&self.details[..]);
        serde_json::to_vec(&Envelope {
            error: Body {
                code: &self.code,
                message: &self.message,
                details,
            },
        })
        .expect("error envelope serializes")
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status)
            .content_type("application/json")
            .body(self.body())
    }
}

/// `GET` error responses must not be cached (research R7).
pub fn no_store_on_get_errors<B>(res: &mut ServiceResponse<B>) {
    let method = res.request().method();
    if (method == Method::GET || method == Method::HEAD)
        && (res.status().is_client_error() || res.status().is_server_error())
    {
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes() {
        let e = ApiError::not_found(Resource::Species, "trex01");
        assert_eq!(e.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = serde_json::from_slice(&e.body()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "error": { "code": "SPECIES_NOT_FOUND", "message": "No species found with id 'trex01'." } })
        );
        let e = ApiError::validation(vec![FieldError::new("id", "bad")]);
        let v: serde_json::Value = serde_json::from_slice(&e.body()).unwrap();
        assert_eq!(v["error"]["details"][0]["field"], "id");
        assert_eq!(e.code(), "VALIDATION_FAILED");
    }
}
