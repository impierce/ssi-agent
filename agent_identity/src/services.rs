use crate::connection::error::ConnectionError;
use agent_secret_manager::subject::Subject;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use did_manager_consumer::resolver::Resolver;
use identity_credential::domain_linkage::{DomainLinkageConfiguration, JwtDomainLinkageValidator};
use identity_did::DIDUrl;
use identity_did::DID;
use identity_iota::{
    core::{FromJson, ToJson},
    credential::JwtCredentialValidationOptions,
    verification::{
        jwk::Jwk as IotaIdentityJwk,
        jws::{JwsVerifier, SignatureVerificationError, SignatureVerificationErrorKind, VerificationInput},
    },
};
use jsonwebtoken::{crypto::verify, jwk::Jwk as JsonWebTokenJwk, Algorithm, DecodingKey, Validation};
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use reqwest::Client;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;
use url::Url;

/// Identity services.
pub struct IdentityServices {
    pub subject: Arc<Subject>,
    pub client: Client,
    pub resolver: Resolver,
}

impl IdentityServices {
    pub fn new(subject: Arc<Subject>, resolver: Resolver) -> Self {
        Self {
            subject,
            client: Client::new(),
            resolver,
        }
    }

    #[cfg(feature = "test_utils")]
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        let (subject, resolver) = futures::executor::block_on(async { (Subject::new().await, Resolver::new()) });

        Arc::new(Self::new(Arc::new(subject), resolver))
    }

    pub fn now(&self) -> DateTime<Utc> {
        #[cfg(feature = "test_utils")]
        return "2026-03-04T12:00:00Z".parse::<DateTime<Utc>>().unwrap();

        #[cfg(not(feature = "test_utils"))]
        Utc::now()
    }

    pub async fn fetch_credential_issuer_metadata(
        &self,
        issuer_url: &Url,
    ) -> Result<CredentialIssuerMetadata, ConnectionError> {
        let mut url = strip_www(issuer_url.clone())?;
        let path = url.path().trim_end_matches('/');
        url.set_path(&format!("/.well-known/openid-credential-issuer{path}"));

        self.client
            .get(url.as_str())
            .send()
            .await
            .map_err(|e| ConnectionError::CredentialIssuerMetadataFetchFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ConnectionError::CredentialIssuerMetadataFetchFailed(e.to_string()))
    }

    pub async fn fetch_linked_dids(&self, url: &Url) -> Result<(Vec<DIDUrl>, bool), ConnectionError> {
        let config = self.fetch_domain_linkage_configuration(url).await?;
        let linked_dids: Vec<DIDUrl> = config
            .linked_dids()
            .iter()
            .filter_map(|jwt| {
                let claims = get_unverified_jwt_claims(jwt.as_str()).ok()?;
                let did_str = claims
                    .get("sub")
                    .or_else(|| claims.get("iss"))
                    .and_then(|v| v.as_str())?;
                did_str.parse::<DIDUrl>().ok()
            })
            .collect();

        let validator = JwtDomainLinkageValidator::with_signature_verifier(Verifier);
        let url = identity_iota::core::Url::from(url.clone());
        let mut all_valid = true;

        if linked_dids.is_empty() {
            info!("No linked DIDs found in configuration");
            return Ok((linked_dids, false));
        }

        for did in &linked_dids {
            match self.resolver.resolve(did.did().as_str()).await {
                Ok(document) => {
                    if validator
                        .validate_linkage(&document, &config, &url, &JwtCredentialValidationOptions::default())
                        .is_ok()
                    {
                        info!("Domain linkage verified for DID: {}", did);
                    } else {
                        info!("Domain linkage verification failed for DID: {}", did);
                        all_valid = false;
                    }
                }
                Err(e) => {
                    info!("Failed to resolve DID {}: {}", did, e);
                    all_valid = false;
                }
            }
        }

        Ok((linked_dids, all_valid))
    }

    async fn fetch_domain_linkage_configuration(
        &self,
        url: &Url,
    ) -> Result<DomainLinkageConfiguration, ConnectionError> {
        let mut url = strip_www(url.clone())?;
        url.set_path("/.well-known/did-configuration.json");

        info!("Fetching DID configuration from: {url}");

        // Fetch the resource and parse to JSON value (mutable)
        let mut response: serde_json::Value = self
            .client
            .get(url.as_str())
            .send()
            .await
            .map_err(|e| ConnectionError::DIDResolutionFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ConnectionError::DIDResolutionFailed(e.to_string()))?;

        // Remove all non-string values from `linked_dids` (JSON-LD)
        if let serde_json::Value::Object(ref mut root) = response {
            if let Some(serde_json::Value::Array(ref mut linked_dids)) = root.get_mut("linked_dids") {
                linked_dids.retain(|did| matches!(did, serde_json::Value::String(_)));
                info!("Removed non-string values from `linked_dids`");
            }
        }
        // Deserialize to `DomainLinkageConfiguration`
        let config = DomainLinkageConfiguration::from_json_value(response).map_err(|_| {
            ConnectionError::DIDResolutionFailed(
                "failed to deserialize DomainLinkageConfiguration from JSON".to_string(),
                // TODO: Add more detailed error info.
            )
        })?;
        Ok(config)
    }
}

// HELPERS
fn strip_www(mut url: Url) -> Result<Url, ConnectionError> {
    if let Some(host) = url.host_str().map(|h| h.to_string()) {
        if let Some(stripped) = host.strip_prefix("www.") {
            url.set_host(Some(stripped))
                .map_err(|e| ConnectionError::MissingDomain(e.to_string()))?;
        }
    }
    Ok(url)
}

/// Get the claims from a jwt string without performing validation.
fn get_unverified_jwt_claims(jwt: &str) -> Result<serde_json::Value, ConnectionError> {
    jwt.splitn(3, '.')
        .collect::<Vec<&str>>()
        .get(1)
        .cloned()
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

/// This `Verifier` uses `jsonwebtoken` under the hood to verify verification input.
pub struct Verifier;
impl JwsVerifier for Verifier {
    fn verify(&self, input: VerificationInput, public_key: &IotaIdentityJwk) -> Result<(), SignatureVerificationError> {
        use SignatureVerificationErrorKind::*;

        info!(
            "Verifying JWS signature using algorithm '{}' against public key.",
            input.alg
        );

        let algorithm =
            Algorithm::from_str(&input.alg.to_string()).map_err(|_| SignatureVerificationError::new(UnsupportedAlg))?;

        // Convert the `IotaIdentityJwk` first into a `JsonWebTokenJwk` and then into a `DecodingKey`.
        let decoding_key = public_key
            .to_json()
            .ok()
            .and_then(|public_key| JsonWebTokenJwk::from_json(&public_key).ok())
            .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
            .ok_or(SignatureVerificationError::new(KeyDecodingFailure))?;

        let mut validation = Validation::new(algorithm);
        validation.validate_aud = false;
        validation.required_spec_claims.clear();

        match verify(
            &URL_SAFE_NO_PAD.encode(input.decoded_signature),
            &input.signing_input,
            &decoding_key,
            algorithm,
        ) {
            Ok(true) => Ok(()),
            Err(_) | Ok(false) => Err(SignatureVerificationError::new(
                // TODO: more fine-grained error handling?
                InvalidSignature,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const LINKED_DID_JWT: &str = "eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNI3o2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSJ9.eyJleHAiOjE3NjQ4NzkxMzksImlzcyI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNIiwibmJmIjoxNjA3MTEyNzM5LCJzdWIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9pZGVudGl0eS5mb3VuZGF0aW9uLy53ZWxsLWtub3duL2RpZC1jb25maWd1cmF0aW9uL3YxIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rb1RIc2dOTnJieThKekNOUTFpUkx5VzVRUTZSOFh1dTZBQThpZ0dyTVZQVU0iLCJvcmlnaW4iOiJpZGVudGl0eS5mb3VuZGF0aW9uIn0sImV4cGlyYXRpb25EYXRlIjoiMjAyNS0xMi0wNFQxNDoxMjoxOS0wNjowMCIsImlzc3VhbmNlRGF0ZSI6IjIwMjAtMTItMDRUMTQ6MTI6MTktMDY6MDAiLCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJEb21haW5MaW5rYWdlQ3JlZGVudGlhbCJdfX0.aUFNReA4R5rcX_oYm3sPXqWtso_gjPHnWZsB6pWcGv6m3K8-4JIAvFov3ZTM8HxPOrOL17Qf4vBFdY9oK0HeCQ";
    const TEST_DID: &str = "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM";

    #[test]
    fn test_decode_linked_did_jwt() {
        let jwt = serde_json::json!(LINKED_DID_JWT);
        let claims = get_unverified_jwt_claims(&jwt.to_string()).unwrap();
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
    fn test_www_stripping() {
        let input_url = Url::parse("https://www.example.com/").unwrap();
        let result = strip_www(input_url).unwrap();
        assert_eq!(result, Url::parse("https://example.com/").unwrap());
    }

    #[test]
    fn test_www_stripping_no_www() {
        let input_url = Url::parse("https://example.com/").unwrap();
        let result = strip_www(input_url).unwrap();
        assert_eq!(result, Url::parse("https://example.com/").unwrap());
    }

    #[tokio::test]
    async fn test_fetch_linked_dids_extracts_dids() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/.well-known/did-configuration.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@context": "https://identity.foundation/.well-known/did-configuration/v1",
                "linked_dids": [LINKED_DID_JWT]
            })))
            .mount(&mock_server)
            .await;

        let subject = Arc::new(Subject::new().await);
        let resolver = Resolver::new();
        let services = IdentityServices::new(subject, resolver);

        let issuer_url: Url = mock_server.uri().parse().unwrap();
        let (dids, domain_linkage_valid) = services.fetch_linked_dids(&issuer_url).await.unwrap();

        assert_eq!(dids.len(), 1);
        assert_eq!(dids[0].did().as_str(), TEST_DID);

        // Validation will fail because the origin in the JWT is "identity.foundation" and we are fetching the did from the mockserver.
        assert!(!domain_linkage_valid);
    }

    #[tokio::test]
    async fn test_fetch_linked_dids_empty() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/.well-known/did-configuration.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@context": "https://identity.foundation/.well-known/did-configuration/v1",
                "linked_dids": []
            })))
            .mount(&mock_server)
            .await;

        let subject = Arc::new(Subject::new().await);
        let resolver = Resolver::new();
        let services = IdentityServices::new(subject, resolver);

        let issuer_url: Url = mock_server.uri().parse().unwrap();
        let result = services.fetch_linked_dids(&issuer_url).await;

        assert!(result.is_err());
    }
}
