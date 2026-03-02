use crate::connection::error::ConnectionError;
use agent_secret_manager::subject::Subject;
use identity_did::DIDUrl;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use reqwest::Client;
use std::sync::Arc;
use url::Url;

/// Identity services.
pub struct IdentityServices {
    pub subject: Arc<Subject>,
    pub client: Client,
}

impl IdentityServices {
    pub fn new(subject: Arc<Subject>) -> Self {
        Self {
            subject,
            client: Client::new(),
        }
    }

    #[cfg(feature = "test_utils")]
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        Arc::new(Self::new(Arc::new(futures::executor::block_on(async {
            Subject::new().await
        }))))
    }

    pub async fn fetch_credential_issuer_metadata(
        &self,
        credential_issuer_url: &Url,
    ) -> Result<CredentialIssuerMetadata, ConnectionError> {
        let mut wellknown_endpoint = credential_issuer_url.clone();
        wellknown_endpoint.set_path(&format!(
            "/.well-known/openid-credential-issuer{}",
            credential_issuer_url.path()
        ));

        self.client
            .get(wellknown_endpoint.as_str())
            .send()
            .await
            .map_err(|e| ConnectionError::CredentialIssuerMetadataFetchFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ConnectionError::CredentialIssuerMetadataFetchFailed(e.to_string()))
    }

    pub async fn fetch_linked_dids(&self, credential_issuer_url: &Url) -> Result<Vec<String>, ConnectionError> {
        let mut did_configurations_endpoint = credential_issuer_url.clone();
        did_configurations_endpoint.set_path("/.well-known/did-configuration.json");

        let response: serde_json::Value = self
            .client
            .get(did_configurations_endpoint)
            .send()
            .await
            .map_err(|e| ConnectionError::DIDConfigurationResolutionFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ConnectionError::DIDConfigurationResolutionFailed(e.to_string()))?;

        // TODO: Add logic for if the linked_did is a JSON-LD VC.
        let linked_dids: Vec<String> = response
            .get("linked_dids")
            .and_then(|v| v.as_array())
            .ok_or(ConnectionError::DIDConfigurationResolutionFailed(
                "no linked_dids found".to_string(),
            ))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        Ok(linked_dids)
    }

    pub async fn resolve_did_web(&self, credential_issuer_url: &Url) -> Result<DIDUrl, ConnectionError> {
        let mut did_web_url = credential_issuer_url.clone();
        did_web_url.set_path("/.well-known/did.json");

        let response: serde_json::Value = self
            .client
            .get(did_web_url.as_str())
            .send()
            .await
            .map_err(|e| ConnectionError::DIDWebResolutionFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ConnectionError::DIDWebResolutionFailed(e.to_string()))?;

        response
            .get("id")
            .and_then(|id| id.as_str())
            .and_then(|id| id.parse::<DIDUrl>().ok())
            .ok_or(ConnectionError::DIDWebResolutionFailed("missing DID id".to_string()))
    }
}
