//! Error envelope, codes (spec §4.12), and field details (spec §4.5).

use std::borrow::Cow;
use std::fmt;

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;

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

/// An error response: status, code, message, and `422` details.
#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: Cow<'static, str>,
    message: String,
    details: Vec<FieldError>,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        todo!()
    }
    pub fn forbidden() -> Self {
        todo!()
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

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }
    fn error_response(&self) -> HttpResponse {
        todo!()
    }
}
