use agent_secret_manager::{service::Service, subject::Subject};
use async_trait::async_trait;
#[cfg(any(feature = "test_utils", test))]
use mockall::automock;
use std::sync::Arc;

pub struct AuthorizationServices {
    pub signer: Arc<Subject>,
}

impl Service for AuthorizationServices {
    fn new(signer: Arc<Subject>) -> Self {
        Self { signer }
    }
}

#[cfg_attr(any(feature = "test_utils", test), automock)]
#[async_trait]
pub trait OpenId4VpPresentationService: Send + Sync {
    async fn create_openid4vp_presentation_request(&self, state: String) -> anyhow::Result<serde_json::Value>;
    async fn verify_openid4vp_response(&self, openid4vp_response: serde_json::Value) -> anyhow::Result<()>;
}

/// This struct is used to hold the services required for the OAuth2AuthorizationRequest aggregate.
/// Currently we only support the OpenId4VpPresentationService, but more services can be added in the future as needed.
pub struct OAuth2AuthorizationRequestDomainServices {
    pub openid4vp_presentation_service: Box<dyn OpenId4VpPresentationService>,
}

impl OAuth2AuthorizationRequestDomainServices {
    #[must_use]
    pub fn new(openid4vp_presentation_service: Box<dyn OpenId4VpPresentationService>) -> Self {
        Self {
            openid4vp_presentation_service,
        }
    }

    #[must_use]
    pub fn openid4vp_presentation_service(&self) -> &dyn OpenId4VpPresentationService {
        self.openid4vp_presentation_service.as_ref()
    }
}

#[cfg(any(feature = "test_utils", test))]
impl Default for OAuth2AuthorizationRequestDomainServices {
    fn default() -> Self {
        let mut mock_openid4vp_presentation_service = MockOpenId4VpPresentationService::new();
        mock_openid4vp_presentation_service
            .expect_create_openid4vp_presentation_request()
            // Returns a default JSON value for testing purposes
            .returning(|_| Ok(serde_json::json!({})));

        Self {
            openid4vp_presentation_service: Box::new(mock_openid4vp_presentation_service),
        }
    }
}
