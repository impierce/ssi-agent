use crate::connection::error::ConnectionError;
use agent_secret_manager::subject::Subject;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use identity_did::DIDUrl;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use reqwest::Client;
use std::sync::Arc;
use url::Url;

/// Identity services.
pub struct IdentityServices {
    pub subject: Arc<Subject>,
    pub client: Client,
    pub mock_time: Option<chrono::DateTime<chrono::Utc>>,
}

impl IdentityServices {
    pub fn new(subject: Arc<Subject>) -> Self {
        Self {
            subject,
            client: Client::new(),
            mock_time: None,
        }
    }

    #[cfg(feature = "test_utils")]
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        let mut services = Self::new(Arc::new(futures::executor::block_on(async { Subject::new().await })));
        services.mock_time = Some("2026-03-04T12:00:00Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap());
        Arc::new(services)
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.mock_time.unwrap_or_else(Utc::now)
    }

    pub async fn fetch_credential_issuer_metadata(
        &self,
        domain: &Url,
    ) -> Result<CredentialIssuerMetadata, ConnectionError> {
        let domain = &normalize_domain(domain)?;
        let mut wellknown_endpoint = domain.clone();
        if !domain.path().is_empty() {
            wellknown_endpoint.set_path(&format!(".well-known/openid-credential-issuer{}", domain.path()));
        } else {
            wellknown_endpoint.set_path(".well-known/openid-credential-issuer");
        }

        self.client
            .get(wellknown_endpoint.as_str().trim_end_matches('/'))
            .send()
            .await
            .map_err(|e| ConnectionError::CredentialIssuerMetadataFetchFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ConnectionError::CredentialIssuerMetadataFetchFailed(e.to_string()))
    }

    pub async fn fetch_and_resolve_linked_dids(&self, domain: &Url) -> Result<Vec<DIDUrl>, ConnectionError> {
        let domain = &normalize_domain(domain)?;
        let mut did_configurations_endpoint = domain.clone();
        did_configurations_endpoint.set_path(".well-known/did-configuration.json");

        let response: serde_json::Value = self
            .client
            .get(did_configurations_endpoint)
            .send()
            .await
            .map_err(|e| ConnectionError::DIDResolutionFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ConnectionError::DIDResolutionFailed(e.to_string()))?;

        // TODO: Add logic for JSON-LD VC linked_dids.
        let linked_dids: Vec<DIDUrl> = response
            .get("linked_dids")
            .and_then(|v| v.as_array())
            .ok_or(ConnectionError::DIDResolutionFailed("no linked dids found".to_string()))?
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

/// Normalize a domain URL to `https://` without `www.`.
/// e.g. `example.com/path` -> `https://example.com/path`
fn normalize_domain(domain: &Url) -> Result<Url, ConnectionError> {
    let mut url = domain.clone();
    let host = url.host_str();
    if host != Some("127.0.0.1") && host != Some("localhost") {
        let _ = url.set_scheme("https");
    }
    if let Some(host) = url.host_str().map(|h| h.to_string()) {
        if let Some(stripped) = host.strip_prefix("www.") {
            url.set_host(Some(stripped))
                .map_err(|e| ConnectionError::MissingDomain(e.to_string()))?;
        }
    }
    Ok(url)
}

/// Get the claims from a JWT without performing validation.
/// TODO: Validate all claims
fn get_unverified_jwt_claims(jwt: &serde_json::Value) -> Result<serde_json::Value, ConnectionError> {
    jwt.as_str()
        .and_then(|string| string.splitn(3, '.').collect::<Vec<&str>>().get(1).cloned())
        .and_then(|payload| {
            URL_SAFE_NO_PAD
                .decode(payload)
                .ok()
                .and_then(|payload_bytes| serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok())
        })
        .ok_or(ConnectionError::DIDResolutionFailed(
            "Failed to decode JWT claims".to_string(),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // https://identity.foundation/.well-known/did-configuration.json
    const LINKED_DID_JWT: &str = "eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNI3o2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSJ9.eyJleHAiOjE3NjQ4NzkxMzksImlzcyI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNIiwibmJmIjoxNjA3MTEyNzM5LCJzdWIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9pZGVudGl0eS5mb3VuZGF0aW9uLy53ZWxsLWtub3duL2RpZC1jb25maWd1cmF0aW9uL3YxIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rb1RIc2dOTnJieThKekNOUTFpUkx5VzVRUTZSOFh1dTZBQThpZ0dyTVZQVU0iLCJvcmlnaW4iOiJpZGVudGl0eS5mb3VuZGF0aW9uIn0sImV4cGlyYXRpb25EYXRlIjoiMjAyNS0xMi0wNFQxNDoxMjoxOS0wNjowMCIsImlzc3VhbmNlRGF0ZSI6IjIwMjAtMTItMDRUMTQ6MTI6MTktMDY6MDAiLCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJEb21haW5MaW5rYWdlQ3JlZGVudGlhbCJdfX0.aUFNReA4R5rcX_oYm3sPXqWtso_gjPHnWZsB6pWcGv6m3K8-4JIAvFov3ZTM8HxPOrOL17Qf4vBFdY9oK0HeCQ";

    #[test]
    fn test_decode_linked_did_jwt() {
        let jwt = serde_json::json!(LINKED_DID_JWT);
        let claims = get_unverified_jwt_claims(&jwt).unwrap();
        assert_eq!(
            claims["sub"],
            "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM"
        );
        assert_eq!(
            claims["iss"],
            "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM"
        );
    }

    #[test]
    fn test_domain_normalization() {
        let domain = "http://www.thatslife.com/";
        let domain_url = Url::parse(domain).unwrap();
        let normalized_url = normalize_domain(&domain_url).unwrap();
        assert_eq!(normalized_url, Url::parse("https://thatslife.com/").unwrap())
    }
}
