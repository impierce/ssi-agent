use agent_secret_manager::{service::Service, subject::Subject};
use async_trait::async_trait;
use oid4vci::credential_format_profiles::CredentialFormats;
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    pub issuer: Arc<Subject>,
    pub additional_claims_provider: Option<Box<dyn AdditionalClaimsProvider>>,
}

impl IssuanceServices {
    /// Set an `AdditionalClaimsProvider` to include additional claims in issued credentials.
    pub fn with_additional_claims_provider(mut self, provider: Box<dyn AdditionalClaimsProvider>) -> Self {
        self.additional_claims_provider = Some(provider);
        self
    }
}

impl Service for IssuanceServices {
    fn new(issuer: Arc<Subject>) -> Self {
        Self {
            issuer,
            additional_claims_provider: None,
        }
    }
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait AdditionalClaimsProvider: Send + Sync {
    async fn add_credential_claims(
        &self,
        credential_format: &CredentialFormats,
        credential_data: &mut serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>>;
}
