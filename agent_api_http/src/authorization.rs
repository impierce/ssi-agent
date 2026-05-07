use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use http::StatusCode;

pub type SharedAuthorizationChecker = Arc<dyn AuthorizationChecker>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub method: String,
    pub path: String,
}

impl AuthorizationRequest {
    pub fn from_request(request: &Request) -> Self {
        Self {
            method: request.method().to_string(),
            path: request.uri().path().to_string(),
        }
    }
}

#[async_trait]
pub trait AuthorizationChecker: Send + Sync {
    async fn is_authorized(&self, request: &AuthorizationRequest) -> bool;
}

pub struct AllowAllAuthorizationChecker;

#[async_trait]
impl AuthorizationChecker for AllowAllAuthorizationChecker {
    async fn is_authorized(&self, _request: &AuthorizationRequest) -> bool {
        true
    }
}

pub async fn authorize_request(
    State(checker): State<SharedAuthorizationChecker>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let authorization_request = AuthorizationRequest::from_request(&request);

    if checker.is_authorized(&authorization_request).await {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    #[tokio::test]
    async fn allow_all_checker_authorizes_requests() {
        let checker = AllowAllAuthorizationChecker;
        let request = AuthorizationRequest {
            method: "GET".to_string(),
            path: "/v0/credentials".to_string(),
        };

        assert!(checker.is_authorized(&request).await);
    }

    #[test]
    fn authorization_request_captures_method_and_path() {
        let request = Request::builder()
            .method("POST")
            .uri("/v0/credentials?ignored=true")
            .body(Body::empty())
            .unwrap();

        let authorization_request = AuthorizationRequest::from_request(&request);

        assert_eq!(
            authorization_request,
            AuthorizationRequest {
                method: "POST".to_string(),
                path: "/v0/credentials".to_string(),
            }
        );
    }
}
