//! Admin check for writes (spec §4.4, research R4).

use actix_web::HttpRequest;

use super::error::ApiError;

/// Decides whether a request may write. Runs first in every write handler.
pub trait AdminGate: Send + Sync + 'static {
    fn check(&self, req: &HttpRequest) -> Result<(), ApiError>;
}

/// The only gate shipped until the security spec: every write is `401`.
pub struct DenyAll;

impl AdminGate for DenyAll {
    fn check(&self, _req: &HttpRequest) -> Result<(), ApiError> {
        todo!()
    }
}
