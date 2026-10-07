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
        Err(ApiError::unauthorized())
    }
}

#[cfg(test)]
mod tests {
    use actix_web::http::header;
    use actix_web::test::TestRequest;

    use super::*;

    #[test]
    fn deny_all_rejects_every_request_with_401() {
        let requests = [
            TestRequest::post().to_http_request(),
            TestRequest::post()
                .insert_header((header::AUTHORIZATION, "Bearer 0f3c5d9e-random-token"))
                .to_http_request(),
            TestRequest::delete()
                .insert_header((header::AUTHORIZATION, "Basic !!not-a-bearer"))
                .to_http_request(),
            TestRequest::patch()
                .insert_header((header::AUTHORIZATION, "Bearer"))
                .to_http_request(),
        ];
        for req in requests {
            let err = DenyAll.check(&req).unwrap_err();
            assert_eq!(err.status().as_u16(), 401);
            assert_eq!(err.code(), "UNAUTHORIZED");
        }
    }
}
