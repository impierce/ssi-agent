use crate::connection::error::ConnectionError;
use agent_secret_manager::subject::Subject;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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

    pub async fn fetch_and_resolve_linked_dids(
        &self,
        credential_issuer_url: &Url,
    ) -> Result<Vec<DIDUrl>, ConnectionError> {
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
        let linked_dids: Vec<DIDUrl> = response
            .get("linked_dids")
            .and_then(|v| v.as_array())
            .ok_or(ConnectionError::DIDConfigurationResolutionFailed(
                "no linked_dids found".to_string(),
            ))?
            .iter()
            .filter_map(|jwt| {
                let claims = get_unverified_jwt_claims(jwt).ok()?;
                let did_str = claims
                    .get("sub")
                    .or_else(|| claims.get("iss"))
                    .and_then(|v| v.as_str())?;
                did_str.parse::<DIDUrl>().ok()
            })
            .collect();

        Ok(linked_dids)
    }

}
// HELPERS
/// Get the claims from a JWT without performing validation.
fn get_unverified_jwt_claims(jwt: &serde_json::Value) -> Result<serde_json::Value, ConnectionError> {
    jwt.as_str()
        .and_then(|string| string.splitn(3, '.').collect::<Vec<&str>>().get(1).cloned())
        .and_then(|payload| {
            URL_SAFE_NO_PAD
                .decode(payload)
                .ok()
                .and_then(|payload_bytes| serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok())
        })
        .ok_or(ConnectionError::DIDConfigurationResolutionFailed(
            "Failed to decode JWT claims".to_string(),
        ))
}
