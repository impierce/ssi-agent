use crate::connection::{
    aggregate::{LinkedCredentialValidation, LinkedVpValidation, ValidationResult},
    error::ConnectionError,
};
use agent_secret_manager::subject::Subject;
use chrono::{DateTime, Utc};
use identity_credential::domain_linkage::{DomainLinkageConfiguration, JwtDomainLinkageValidator};
use identity_did::{CoreDID, DIDUrl, DID};
use identity_iota::{
    core::{FromJson, Object, ToJson},
    credential::{
        Credential, DecodedJwtPresentation, FailFast, Jwt, JwtCredentialValidationOptions, JwtCredentialValidator,
        JwtCredentialValidatorUtils, JwtPresentationValidationOptions, JwtPresentationValidator, StatusCheck,
    },
    document::CoreDocument,
};
use oid4vc_core::utils::jwt::get_unverified_jwt_claims;
use oid4vc_core::verifier::SignatureVerifier;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use reqwest::{redirect, Client};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::lookup_host;
use tracing::{info, warn};
use url::{Host, Url};

const LINKED_VP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const LINKED_VP_RESPONSE_LIMIT: usize = 5 * 1024 * 1024;

/// A DID extracted from a domain's DID configuration, together with the outcome of its domain
/// linkage verification.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedDid {
    pub did: DIDUrl,
    pub domain_linkage_valid: bool,
    pub domain_linkage_error: Option<String>,
}

impl LinkedDid {
    /// A DID whose domain linkage could not be established.
    pub fn unverified(did: DIDUrl, error: impl Into<String>) -> Self {
        Self {
            did,
            domain_linkage_valid: false,
            domain_linkage_error: Some(error.into()),
        }
    }
}

/// Identity services.
pub struct IdentityServices {
    pub subject: Arc<Subject>,
    pub client: Client,
    /// Whether linked VP endpoints on the local network may use `http`. Only `true` in local
    /// development builds; public HTTP and sensitive address ranges remain blocked. See
    /// `docs/adr/0002-allow-localhost-http-fallback-for-local-testing.md`.
    pub allow_local_network_vp_endpoints: bool,
}

impl IdentityServices {
    pub fn new(subject: Arc<Subject>) -> Self {
        Self {
            subject,
            client: Client::new(),
            allow_local_network_vp_endpoints: cfg!(feature = "allow-localhost"),
        }
    }

    #[cfg(feature = "test_utils")]
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        let subject = futures::executor::block_on(async { Subject::new().await });

        // Tests drive linked VP endpoints from a mock server on `127.0.0.1`, which the outbound
        // policy rejects by default.
        Arc::new(Self {
            allow_local_network_vp_endpoints: true,
            ..Self::new(Arc::new(subject))
        })
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
        let mut url = issuer_url.clone();
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

    pub async fn fetch_linked_dids(&self, url: &Url) -> Result<Vec<LinkedDid>, ConnectionError> {
        // TODO: This essentially disables domain linkage fetching because HTTPS is strictly
        // required by `DomainLinkageConfiguration::from_json_value`. When running locally
        // with HTTP, the fetch fails and we gracefully default to no linked DIDs.
        // See `docs/adr/0002-allow-localhost-http-fallback-for-local-testing.md` for more context and the future plan
        // to use `rcgen`.
        #[cfg(feature = "allow-localhost")]
        let config = match self.fetch_domain_linkage_configuration(url).await {
            Ok(config) => config,
            Err(_) => return Ok(vec![]),
        };

        #[cfg(not(feature = "allow-localhost"))]
        let config = self.fetch_domain_linkage_configuration(url).await?;
        let linked_dids: Vec<DIDUrl> = config
            .linked_dids()
            .iter()
            .filter_map(|jwt| {
                let jwt_value = jwt.to_json_value().ok()?;
                let claims = get_unverified_jwt_claims(&jwt_value).ok()?;
                let did_str = claims
                    .get("sub")
                    .or_else(|| claims.get("iss"))
                    .and_then(|v| v.as_str())?;
                did_str.parse::<DIDUrl>().ok()
            })
            .collect();

        if linked_dids.is_empty() {
            info!("No linked DIDs found in configuration");
            return Ok(vec![]);
        }

        let validator = JwtDomainLinkageValidator::with_signature_verifier(SignatureVerifier);
        let url = identity_iota::core::Url::from(url.clone());
        let mut results = Vec::with_capacity(linked_dids.len());

        // Linkage is tracked per DID rather than aggregated into a single boolean: a single failing
        // DID must not make the other linked DIDs of the same domain indistinguishable from it.
        for did in linked_dids {
            let result = match self.subject.resolver.resolve(did.did().as_str()).await {
                Ok(document) => {
                    match validator.validate_linkage(
                        &document,
                        &config,
                        &url,
                        &JwtCredentialValidationOptions::default(),
                    ) {
                        Ok(()) => {
                            info!("Domain linkage verified for DID: {did}");
                            LinkedDid {
                                did,
                                domain_linkage_valid: true,
                                domain_linkage_error: None,
                            }
                        }
                        Err(error) => {
                            info!("Domain linkage verification failed for DID {did}: {error}");
                            LinkedDid::unverified(did, format!("Domain linkage verification failed: {error}"))
                        }
                    }
                }
                Err(error) => {
                    info!("Failed to resolve DID {did}: {error}");
                    LinkedDid::unverified(did, format!("Failed to resolve DID: {error}"))
                }
            };
            results.push(result);
        }

        Ok(results)
    }

    pub async fn fetch_linked_vp_validations(&self, linked_dids: &[LinkedDid]) -> Vec<LinkedVpValidation> {
        let mut validations = Vec::new();

        // Presentations are fetched regardless of `domain_linkage_valid`: a DID whose linkage could
        // not be established is not proof of forgery, and the resulting validation is surfaced to
        // the user flagged as unlinked rather than being silently withheld. The DID Document is
        // never trusted to direct outbound requests — see `resolve_outbound_url`.
        for linked_did in linked_dids {
            let did = &linked_did.did;
            let document = match self.subject.resolver.resolve(did.did().as_str()).await {
                Ok(document) => document,
                Err(error) => {
                    warn!("Failed to resolve DID {did} for linked VP validation: {error}");
                    continue;
                }
            };

            for url in linked_verifiable_presentation_urls(&document) {
                validations.push(
                    self.validate_linked_verifiable_presentation(linked_did, &document, url)
                        .await,
                );
            }
        }

        validations
    }

    async fn validate_linked_verifiable_presentation(
        &self,
        linked_did: &LinkedDid,
        holder_document: &CoreDocument,
        url: Url,
    ) -> LinkedVpValidation {
        let last_validated_at = self.now();
        let did = linked_did.did.to_string();

        let (valid, error, credentials) = match self.validated_credentials(holder_document, &url).await {
            Ok(credentials) => {
                let invalid = credentials.iter().filter(|c| !c.result.valid).count();
                let error = (invalid > 0).then(|| {
                    format!(
                        "{invalid} of {} embedded credentials failed validation",
                        credentials.len()
                    )
                });
                (invalid == 0, error, credentials)
            }
            Err(error) => (false, Some(error.to_string()), Vec::new()),
        };

        LinkedVpValidation {
            url: url.into(),
            did,
            domain_linkage_valid: linked_did.domain_linkage_valid,
            result: ValidationResult {
                valid,
                error,
                last_validated_at,
            },
            credentials,
        }
    }

    async fn validated_credentials(
        &self,
        holder_document: &CoreDocument,
        url: &Url,
    ) -> anyhow::Result<Vec<LinkedCredentialValidation>> {
        let presentation_jwt = self.fetch_linked_verifiable_presentation(url).await?;
        let presentation: DecodedJwtPresentation<Jwt> =
            JwtPresentationValidator::with_signature_verifier(SignatureVerifier)
                .validate(
                    &Jwt::from(presentation_jwt),
                    holder_document,
                    &JwtPresentationValidationOptions::default(),
                )
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let validator = JwtCredentialValidator::with_signature_verifier(SignatureVerifier);
        let options = JwtCredentialValidationOptions::new().status_check(StatusCheck::SkipUnsupported);
        let last_validated_at = self.now();
        let mut credentials = Vec::new();

        // Every embedded credential is reported with its own outcome. A credential that fails to
        // validate neither disappears from the response nor marks the others as untrustworthy; the
        // caller derives the presentation-level verdict from these per-credential results.
        for credential_jwt in presentation.presentation.verifiable_credential {
            credentials.push(
                self.validate_linked_credential(&validator, &options, credential_jwt, last_validated_at)
                    .await,
            );
        }

        Ok(credentials)
    }

    async fn validate_linked_credential(
        &self,
        validator: &JwtCredentialValidator<SignatureVerifier>,
        options: &JwtCredentialValidationOptions,
        credential_jwt: Jwt,
        last_validated_at: DateTime<Utc>,
    ) -> LinkedCredentialValidation {
        let jwt = credential_jwt.as_str().to_owned();
        // Decoded up front so content that fails validation can still be shown to the user.
        let unverified = unverified_credential(&credential_jwt);

        let invalid = |error: String| LinkedCredentialValidation {
            credential: unverified.clone(),
            jwt: jwt.clone(),
            result: ValidationResult {
                valid: false,
                error: Some(error),
                last_validated_at,
            },
        };

        let issuer: CoreDID = match JwtCredentialValidatorUtils::extract_issuer_from_jwt(&credential_jwt) {
            Ok(issuer) => issuer,
            Err(error) => {
                warn!("Failed to extract linked credential issuer: {error}");
                return invalid(format!("Failed to extract credential issuer: {error}"));
            }
        };
        let issuer_document = match self.subject.resolver.resolve(issuer.as_str()).await {
            Ok(document) => document,
            Err(error) => {
                warn!("Failed to resolve linked credential issuer {issuer}: {error}");
                return invalid(format!("Failed to resolve credential issuer '{issuer}': {error}"));
            }
        };

        match validator.validate::<_, Object>(&credential_jwt, &issuer_document, options, FailFast::FirstError) {
            Ok(decoded) => LinkedCredentialValidation {
                credential: Some(decoded.credential),
                jwt,
                result: ValidationResult {
                    valid: true,
                    error: None,
                    last_validated_at,
                },
            },
            Err(error) => {
                warn!("Failed to validate linked credential issued by {issuer}: {error}");
                invalid(format!("Failed to validate credential issued by '{issuer}': {error}"))
            }
        }
    }

    /// Fetches a linked verifiable presentation under an outbound-network policy.
    ///
    /// SECURITY: `url` originates from a `serviceEndpoint` in a resolved DID Document and is
    /// therefore chosen by whoever published that document. Without these checks a hostile DID
    /// could make the agent issue requests to link-local or otherwise sensitive services on its
    /// behalf (SSRF). Redirects are not followed, and the vetted DNS results are pinned to the
    /// request so they cannot change between validation and connection.
    async fn fetch_linked_verifiable_presentation(&self, url: &Url) -> anyhow::Result<String> {
        let addresses = resolve_outbound_url(url, self.allow_local_network_vp_endpoints)
            .await
            .map_err(|error| anyhow::anyhow!("Refused to fetch linked VP from '{url}': {error}"))?;
        let client = linked_vp_client(url, &addresses)?;

        let response = client.get(url.as_str()).send().await?;
        if response.status().is_redirection() {
            anyhow::bail!(
                "Linked VP endpoint '{url}' responded with a redirect, which is not followed for security reasons"
            );
        }

        read_limited_linked_vp(response.error_for_status()?).await
    }

    async fn fetch_domain_linkage_configuration(
        &self,
        url: &Url,
    ) -> Result<DomainLinkageConfiguration, ConnectionError> {
        let mut url = url.clone();
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

/// Decodes the `vc` claim of a credential JWT **without verifying its signature**.
///
/// Used so that a credential which failed validation can still be surfaced to the user, clearly
/// flagged as invalid. The result must never be treated as verified content.
fn unverified_credential(credential_jwt: &Jwt) -> Option<Credential> {
    let jwt_value = credential_jwt.to_json_value().ok()?;
    let claims = get_unverified_jwt_claims(&jwt_value).ok()?;
    let mut credential = claims.get("vc")?.as_object()?.clone();

    // VC-JWT hoists a handful of credential fields into registered JWT claims (VC Data Model 1.1
    // §6.3.1), so they have to be folded back in before the credential can be parsed.
    for (field, claim) in [("issuer", "iss"), ("id", "jti")] {
        if !credential.contains_key(field) {
            if let Some(value) = claims.get(claim) {
                credential.insert(field.to_owned(), value.clone());
            }
        }
    }
    if !credential.contains_key("issuanceDate") {
        if let Some(value) = claims.get("nbf").or_else(|| claims.get("iat")).and_then(rfc3339_claim) {
            credential.insert("issuanceDate".to_owned(), value);
        }
    }
    if !credential.contains_key("expirationDate") {
        if let Some(value) = claims.get("exp").and_then(rfc3339_claim) {
            credential.insert("expirationDate".to_owned(), value);
        }
    }

    Credential::from_json_value(serde_json::Value::Object(credential)).ok()
}

/// Renders a numeric JWT timestamp claim as the RFC 3339 string a credential expects.
fn rfc3339_claim(claim: &serde_json::Value) -> Option<serde_json::Value> {
    let timestamp = DateTime::from_timestamp(claim.as_i64()?, 0)?;
    Some(serde_json::Value::String(
        timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    ))
}

/// Resolves and vets every address that may be used to fetch a linked verifiable presentation.
async fn resolve_outbound_url(url: &Url, allow_local_network: bool) -> Result<Vec<SocketAddr>, String> {
    if !matches!(url.scheme(), "https" | "http") {
        return Err(format!("scheme '{}' is not permitted", url.scheme()));
    }

    let host = url.host().ok_or_else(|| "URL has no host".to_string())?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = match host {
        Host::Domain(host) => lookup_host((host, port))
            .await
            .map_err(|error| format!("failed to resolve host '{host}': {error}"))?
            .collect::<Vec<_>>(),
        Host::Ipv4(ip) => vec![SocketAddr::new(IpAddr::V4(ip), port)],
        Host::Ipv6(ip) => vec![SocketAddr::new(IpAddr::V6(ip), port)],
    };

    if addresses.is_empty() {
        return Err(format!("host '{host}' did not resolve to any address"));
    }

    for address in &addresses {
        let ip = address.ip();
        let allowed = if is_globally_routable(ip) {
            url.scheme() == "https"
        } else if is_local_network(ip) {
            allow_local_network
        } else {
            false
        };

        if !allowed {
            return Err(format!(
                "scheme '{}' is not permitted for resolved address {ip}",
                url.scheme()
            ));
        }
    }

    Ok(addresses)
}

/// Builds a per-request client whose resolver is pinned to addresses that passed policy checks.
fn linked_vp_client(url: &Url, addresses: &[SocketAddr]) -> anyhow::Result<Client> {
    let mut builder = Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(LINKED_VP_REQUEST_TIMEOUT)
        // A configured proxy would resolve the target independently and bypass the pinned result.
        .no_proxy();

    if let Some(Host::Domain(host)) = url.host() {
        builder = builder.resolve_to_addrs(host, addresses);
    }

    builder.build().map_err(Into::into)
}

async fn read_limited_linked_vp(mut response: reqwest::Response) -> anyhow::Result<String> {
    if response
        .content_length()
        .is_some_and(|length| length > LINKED_VP_RESPONSE_LIMIT as u64)
    {
        anyhow::bail!("Linked VP response exceeds the {LINKED_VP_RESPONSE_LIMIT}-byte limit");
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > LINKED_VP_RESPONSE_LIMIT {
            anyhow::bail!("Linked VP response exceeds the {LINKED_VP_RESPONSE_LIMIT}-byte limit");
        }
        body.extend_from_slice(&chunk);
    }

    String::from_utf8(body).map_err(|error| anyhow::anyhow!("Linked VP response is not valid UTF-8: {error}"))
}

/// Whether `ip` is a public address, i.e. not one that could reach the agent's own host or network.
///
/// Hand-rolled because `IpAddr::is_global` is still unstable.
fn is_globally_routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_multicast()
                || ip.is_unspecified()
                // 0.0.0.0/8 (current network)
                || a == 0
                // 100.64.0.0/10 (carrier-grade NAT)
                || (a == 100 && (b & 0xc0) == 0x40)
                // 192.0.0.0/24 (IETF protocol assignments)
                || (a == 192 && b == 0 && ip.octets()[2] == 0)
                // 240.0.0.0/4 (reserved)
                || a >= 240)
        }
        IpAddr::V6(ip) => {
            if ip.is_loopback() {
                return false;
            }
            if let Some(ipv4) = ip.to_ipv4() {
                return is_globally_routable(IpAddr::V4(ipv4));
            }
            let first = ip.segments()[0];
            !(ip.is_multicast()
                || ip.is_unspecified()
                // fc00::/7 (unique local)
                || (first & 0xfe00) == 0xfc00
                // fec0::/10 (deprecated site-local)
                || (first & 0xffc0) == 0xfec0
                // fe80::/10 (link-local)
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

fn is_local_network(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => {
            if ip.is_loopback() {
                return true;
            }
            if let Some(ipv4) = ip.to_ipv4() {
                return is_local_network(IpAddr::V4(ipv4));
            }
            let first = ip.segments()[0];
            (first & 0xfe00) == 0xfc00
        }
    }
}

fn linked_verifiable_presentation_urls(document: &CoreDocument) -> Vec<Url> {
    document
        .service()
        .iter()
        .filter(|service| service.type_().contains("LinkedVerifiablePresentation"))
        .filter_map(|service| service.service_endpoint().to_json_value().ok())
        .flat_map(|endpoint| match endpoint {
            serde_json::Value::String(url) => vec![url],
            serde_json::Value::Array(urls) => urls
                .into_iter()
                .filter_map(|url| url.as_str().map(ToOwned::to_owned))
                .collect(),
            _ => Vec::new(),
        })
        .filter_map(|url| {
            url.parse()
                .inspect_err(|error| warn!("Failed to parse linked VP URL '{url}': {error}"))
                .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_core::{common::Object, convert::FromJson};
    use identity_credential::{
        credential::{Credential, CredentialBuilder, Jwt, Subject as CredentialSubject},
        presentation::{JwtPresentationOptions, PresentationBuilder},
    };
    use identity_document::document::CoreDocument;
    use identity_iota::{
        storage::{JwkDocumentExt, JwsSignatureOptions, KeyIdMemstore, Storage},
        verification::{jws::JwsAlgorithm, MethodScope},
    };
    use identity_storage::JwkMemStore;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const LINKED_DID_JWT: &str = "eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNI3o2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSJ9.eyJleHAiOjE3NjQ4NzkxMzksImlzcyI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNIiwibmJmIjoxNjA3MTEyNzM5LCJzdWIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9pZGVudGl0eS5mb3VuZGF0aW9uLy53ZWxsLWtub3duL2RpZC1jb25maWd1cmF0aW9uL3YxIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rb1RIc2dOTnJieThKekNOUTFpUkx5VzVRUTZSOFh1dTZBQThpZ0dyTVZQVU0iLCJvcmlnaW4iOiJpZGVudGl0eS5mb3VuZGF0aW9uIn0sImV4cGlyYXRpb25EYXRlIjoiMjAyNS0xMi0wNFQxNDoxMjoxOS0wNjowMCIsImlzc3VhbmNlRGF0ZSI6IjIwMjAtMTItMDRUMTQ6MTI6MTktMDY6MDAiLCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJEb21haW5MaW5rYWdlQ3JlZGVudGlhbCJdfX0.aUFNReA4R5rcX_oYm3sPXqWtso_gjPHnWZsB6pWcGv6m3K8-4JIAvFov3ZTM8HxPOrOL17Qf4vBFdY9oK0HeCQ";
    const TEST_DID: &str = "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM";

    /// A linked DID whose domain linkage is treated as established, so that tests can exercise the
    /// linked VP path in isolation from domain linkage.
    fn verified_linked_did(did: &str) -> LinkedDid {
        LinkedDid {
            did: did.parse().unwrap(),
            domain_linkage_valid: true,
            domain_linkage_error: None,
        }
    }

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
    fn unavailable_credential_is_omitted_from_json() {
        let validation = LinkedCredentialValidation {
            credential: None,
            jwt: "invalid.jwt".to_string(),
            result: ValidationResult {
                valid: false,
                error: Some("credential could not be decoded".to_string()),
                last_validated_at: "2026-03-04T12:00:00Z".parse().unwrap(),
            },
        };

        let json = serde_json::to_value(validation).unwrap();
        assert!(json.get("credential").is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_returns_validated_credentials() {
        let mock_server = MockServer::start().await;
        let holder_did = format!("did:web:localhost%3A{}", mock_server.address().port());
        let mut holder_document = CoreDocument::from_json_value(serde_json::json!({
            "@context": "https://www.w3.org/ns/did/v1",
            "id": holder_did
        }))
        .unwrap();
        let storage = Storage::new(JwkMemStore::new(), KeyIdMemstore::new());
        let fragment = holder_document
            .generate_method(
                &storage,
                JwkMemStore::ED25519_KEY_TYPE,
                JwsAlgorithm::EdDSA,
                None,
                MethodScope::assertion_method(),
            )
            .await
            .unwrap();

        let credential: Credential = CredentialBuilder::new(Object::new())
            .issuer(holder_document.id().to_url())
            .type_("Endorsement")
            .subject(
                CredentialSubject::from_json_value(serde_json::json!({
                    "id": holder_document.id().as_str(),
                    "Content": "Awesome collaboration!"
                }))
                .unwrap(),
            )
            .build()
            .unwrap();
        let credential_jwt = holder_document
            .create_credential_jwt(&credential, &storage, &fragment, &JwsSignatureOptions::default(), None)
            .await
            .unwrap();
        let presentation = PresentationBuilder::new(holder_document.id().to_url().into(), Object::new())
            .credential(credential_jwt)
            .build()
            .unwrap();
        let presentation_jwt: Jwt = holder_document
            .create_presentation_jwt(
                &presentation,
                &storage,
                &fragment,
                &JwsSignatureOptions::default(),
                &JwtPresentationOptions::default(),
            )
            .await
            .unwrap();

        let presentation_url = format!("{}/linked-vp", mock_server.uri());
        let linked_vp_service = identity_document::service::Service::builder(Default::default())
            .id(format!("{holder_did}#linked-verifiable-presentation-service")
                .parse()
                .unwrap())
            .type_("LinkedVerifiablePresentation")
            .service_endpoint(presentation_url.parse::<identity_core::common::Url>().unwrap())
            .build()
            .unwrap();
        holder_document.insert_service(linked_vp_service).unwrap();

        Mock::given(method("GET"))
            .and(path("/.well-known/did.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&holder_document))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/linked-vp"))
            .respond_with(ResponseTemplate::new(200).set_body_string(presentation_jwt.as_str()))
            .mount(&mock_server)
            .await;

        let services = IdentityServices::default();
        let resolved_document = services.subject.resolver.resolve(&holder_did).await.unwrap();
        assert_eq!(linked_verifiable_presentation_urls(&resolved_document).len(), 1);
        let validations = services
            .fetch_linked_vp_validations(&[verified_linked_did(&holder_did)])
            .await;

        assert_eq!(validations.len(), 1);
        assert!(validations[0].result.valid);
        assert!(validations[0].domain_linkage_valid);
        assert_eq!(validations[0].did, holder_did);
        assert_eq!(validations[0].credentials.len(), 1);
        assert!(validations[0].credentials[0].result.valid);
        assert_eq!(validations[0].credentials[0].credential, Some(credential));

        let unverified = LinkedDid::unverified(holder_did.parse().unwrap(), "Domain linkage failed");
        let unverified_validations = services.fetch_linked_vp_validations(&[unverified]).await;

        assert_eq!(unverified_validations.len(), 1);
        assert!(!unverified_validations[0].domain_linkage_valid);
        assert!(unverified_validations[0].result.valid);
        assert!(unverified_validations[0].result.error.is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_reports_each_embedded_credential_separately() {
        let mock_server = MockServer::start().await;
        let holder_did = format!("did:web:localhost%3A{}", mock_server.address().port());
        let mut holder_document = CoreDocument::from_json_value(serde_json::json!({
            "@context": "https://www.w3.org/ns/did/v1",
            "id": holder_did
        }))
        .unwrap();
        let storage = Storage::new(JwkMemStore::new(), KeyIdMemstore::new());
        let fragment = holder_document
            .generate_method(
                &storage,
                JwkMemStore::ED25519_KEY_TYPE,
                JwsAlgorithm::EdDSA,
                None,
                MethodScope::assertion_method(),
            )
            .await
            .unwrap();

        let credential: Credential = CredentialBuilder::new(Object::new())
            .issuer(holder_document.id().to_url())
            .type_("Endorsement")
            .subject(
                CredentialSubject::from_json_value(serde_json::json!({
                    "id": holder_document.id().as_str(),
                    "Content": "Awesome collaboration!"
                }))
                .unwrap(),
            )
            .build()
            .unwrap();
        let credential_jwt = holder_document
            .create_credential_jwt(&credential, &storage, &fragment, &JwsSignatureOptions::default(), None)
            .await
            .unwrap();

        // Same credential with a corrupted signature: the issuer still resolves, but the signature
        // no longer verifies.
        let tampered_jwt = {
            let (payload, signature) = credential_jwt.as_str().rsplit_once('.').unwrap();
            let mut signature: Vec<char> = signature.chars().collect();
            signature[0] = if signature[0] == 'A' { 'B' } else { 'A' };
            Jwt::from(format!("{payload}.{}", signature.into_iter().collect::<String>()))
        };

        let presentation = PresentationBuilder::new(holder_document.id().to_url().into(), Object::new())
            .credential(credential_jwt)
            .credential(tampered_jwt.clone())
            .build()
            .unwrap();
        let presentation_jwt: Jwt = holder_document
            .create_presentation_jwt(
                &presentation,
                &storage,
                &fragment,
                &JwsSignatureOptions::default(),
                &JwtPresentationOptions::default(),
            )
            .await
            .unwrap();

        let presentation_url = format!("{}/linked-vp", mock_server.uri());
        let linked_vp_service = identity_document::service::Service::builder(Default::default())
            .id(format!("{holder_did}#linked-verifiable-presentation-service")
                .parse()
                .unwrap())
            .type_("LinkedVerifiablePresentation")
            .service_endpoint(presentation_url.parse::<identity_core::common::Url>().unwrap())
            .build()
            .unwrap();
        holder_document.insert_service(linked_vp_service).unwrap();

        Mock::given(method("GET"))
            .and(path("/.well-known/did.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&holder_document))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/linked-vp"))
            .respond_with(ResponseTemplate::new(200).set_body_string(presentation_jwt.as_str()))
            .mount(&mock_server)
            .await;

        let services = IdentityServices::default();
        let validations = services
            .fetch_linked_vp_validations(&[verified_linked_did(&holder_did)])
            .await;

        assert_eq!(validations.len(), 1);

        // A partially valid presentation must not be reported as valid ...
        assert!(!validations[0].result.valid);
        assert_eq!(
            validations[0].result.error.as_deref(),
            Some("1 of 2 embedded credentials failed validation")
        );

        // ... while both credentials are still returned, each with its own outcome.
        assert_eq!(validations[0].credentials.len(), 2);
        assert!(validations[0].credentials[0].result.valid);
        assert_eq!(validations[0].credentials[0].credential, Some(credential));

        assert!(!validations[0].credentials[1].result.valid);
        assert!(validations[0].credentials[1].result.error.is_some());
        // Content is still surfaced, decoded without verifying the signature.
        assert_eq!(
            validations[0].credentials[1].credential.as_ref().unwrap().types,
            validations[0].credentials[0].credential.as_ref().unwrap().types
        );
        assert_eq!(validations[0].credentials[1].jwt, tampered_jwt.as_str());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_keeps_linkage_separate_from_vp_errors() {
        let mock_server = MockServer::start().await;
        let holder_did = format!("did:web:localhost%3A{}", mock_server.address().port());
        let presentation_url = format!("{}/linked-vp", mock_server.uri());
        let holder_document = serde_json::json!({
            "@context": "https://www.w3.org/ns/did/v1",
            "id": holder_did,
            "service": [{
                "id": format!("{holder_did}#linked-verifiable-presentation-service"),
                "type": "LinkedVerifiablePresentation",
                "serviceEndpoint": presentation_url
            }]
        });

        Mock::given(method("GET"))
            .and(path("/.well-known/did.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(holder_document))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/linked-vp"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-a-jwt"))
            .mount(&mock_server)
            .await;

        let services = IdentityServices::default();
        let unverified = LinkedDid::unverified(holder_did.parse().unwrap(), "Failed to resolve DID");
        let validations = services.fetch_linked_vp_validations(&[unverified]).await;

        // The presentation is still discovered and returned, flagged as not linked to the domain.
        assert_eq!(validations.len(), 1);
        assert!(!validations[0].domain_linkage_valid);
        assert!(!validations[0].result.valid);
        let error = validations[0].result.error.as_deref().unwrap();
        assert!(!error.contains("not verifiably linked"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_reports_invalid_presentations() {
        let mock_server = MockServer::start().await;
        let holder_did = format!("did:web:localhost%3A{}", mock_server.address().port());
        let presentation_url = format!("{}/linked-vp", mock_server.uri());
        let holder_document = serde_json::json!({
            "@context": "https://www.w3.org/ns/did/v1",
            "id": holder_did,
            "service": [{
                "id": format!("{holder_did}#linked-verifiable-presentation-service"),
                "type": "LinkedVerifiablePresentation",
                "serviceEndpoint": presentation_url
            }]
        });

        Mock::given(method("GET"))
            .and(path("/.well-known/did.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(holder_document))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/linked-vp"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-a-jwt"))
            .mount(&mock_server)
            .await;

        let services = IdentityServices::default();
        let validations = services
            .fetch_linked_vp_validations(&[verified_linked_did(&holder_did)])
            .await;

        assert_eq!(validations.len(), 1);
        assert!(!validations[0].result.valid);
        assert!(validations[0].result.error.is_some());
        assert!(validations[0].credentials.is_empty());
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
        let services = IdentityServices::new(subject);

        let issuer_url: Url = mock_server.uri().parse().unwrap();
        let dids = services.fetch_linked_dids(&issuer_url).await.unwrap();

        assert_eq!(dids.len(), 1);
        assert_eq!(dids[0].did.did().as_str(), TEST_DID);

        // Validation will fail because the origin in the JWT is "identity.foundation" and we are fetching the did from the mockserver.
        assert!(!dids[0].domain_linkage_valid);
        assert!(dids[0].domain_linkage_error.is_some());
    }

    #[tokio::test]
    #[cfg(not(feature = "allow-localhost"))]
    async fn test_fetch_linked_dids_empty_fails() {
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
        let services = IdentityServices::new(subject);

        let issuer_url: Url = mock_server.uri().parse().unwrap();
        let result = services.fetch_linked_dids(&issuer_url).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn outbound_policy_rejects_local_destinations_by_default() {
        for url in [
            "file:///etc/passwd",             // non-http scheme
            "https://127.0.0.1/vp",           // loopback
            "https://localhost/vp",           // loopback by name
            "https://169.254.169.254/latest", // cloud metadata service
            "https://10.0.0.1/vp",            // private
            "https://192.168.1.1/vp",         // private
            "https://[::1]/vp",               // IPv6 loopback
            "https://[fd00::1]/vp",           // IPv6 unique local
        ] {
            let url: Url = url.parse().unwrap();
            assert!(
                resolve_outbound_url(&url, false).await.is_err(),
                "expected '{url}' to be rejected"
            );
        }
    }

    #[tokio::test]
    async fn outbound_policy_allows_local_network_destinations_when_configured() {
        for url in [
            "http://127.0.0.1:3033/vp",
            "http://10.0.0.1/vp",
            "http://192.168.1.1/vp",
            "http://[::1]/vp",
            "http://[fd00::1]/vp",
        ] {
            let url: Url = url.parse().unwrap();
            assert!(resolve_outbound_url(&url, false).await.is_err());
            assert!(resolve_outbound_url(&url, true).await.is_ok());
        }
    }

    #[tokio::test]
    async fn local_network_policy_still_rejects_public_http_and_sensitive_destinations() {
        for url in [
            "http://93.184.216.34/vp",
            "http://169.254.169.254/latest",
            "https://169.254.169.254/latest",
            "http://[fe80::1]/vp",
            "https://[fe80::1]/vp",
        ] {
            let url: Url = url.parse().unwrap();
            assert!(
                resolve_outbound_url(&url, true).await.is_err(),
                "expected '{url}' to be rejected"
            );
        }
    }

    #[tokio::test]
    async fn outbound_policy_allows_public_https() {
        let url: Url = "https://93.184.216.34/vp".parse().unwrap();

        assert!(resolve_outbound_url(&url, false).await.is_ok());
    }

    #[tokio::test]
    async fn linked_vp_client_uses_pinned_dns_results() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/linked-vp"))
            .respond_with(ResponseTemplate::new(200).set_body_string("presentation"))
            .mount(&mock_server)
            .await;

        let url: Url = format!("http://linked-vp.invalid:{}/linked-vp", mock_server.address().port())
            .parse()
            .unwrap();
        let client = linked_vp_client(&url, &[*mock_server.address()]).unwrap();
        let response = client.get(url).send().await.unwrap();

        assert_eq!(read_limited_linked_vp(response).await.unwrap(), "presentation");
    }

    #[tokio::test]
    async fn linked_vp_response_size_is_limited() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/linked-vp"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'a'; LINKED_VP_RESPONSE_LIMIT + 1]))
            .mount(&mock_server)
            .await;

        let services = IdentityServices::default();
        let url: Url = format!("{}/linked-vp", mock_server.uri()).parse().unwrap();

        let error = services.fetch_linked_verifiable_presentation(&url).await.unwrap_err();
        assert!(error.to_string().contains("response exceeds"));
    }

    #[tokio::test]
    async fn linked_vp_redirects_are_not_followed() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/linked-vp"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/redirected"))
            .mount(&mock_server)
            .await;

        let services = IdentityServices::default();
        let url: Url = format!("{}/linked-vp", mock_server.uri()).parse().unwrap();

        let error = services.fetch_linked_verifiable_presentation(&url).await.unwrap_err();
        assert!(error.to_string().contains("responded with a redirect"));
    }

    #[test]
    fn globally_routable_addresses_are_accepted() {
        for ip in ["93.184.216.34", "8.8.8.8", "2606:2800:220:1:248:1893:25c8:1946"] {
            assert!(
                is_globally_routable(ip.parse().unwrap()),
                "expected '{ip}' to be allowed"
            );
        }

        // IPv4-mapped IPv6 must be judged by the address it maps to.
        assert!(!is_globally_routable("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_globally_routable("::10.0.0.1".parse().unwrap()));
        assert!(!is_globally_routable("0.0.0.1".parse().unwrap()));
    }

    #[test]
    fn rustsec_2026_0258_dependency_paths_remain_scoped() {
        let lockfile = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.lock"));

        let vulnerable_reqwest = locked_package(lockfile, "reqwest", "0.11.27");
        assert!(vulnerable_reqwest.contains("\"h2 0.3.27\""));
        assert!(vulnerable_reqwest.contains("\"hyper 0.14.32\""));

        let vulnerable_hyper = locked_package(lockfile, "hyper", "0.14.32");
        assert!(vulnerable_hyper.contains("\"h2 0.3.27\""));

        for (name, version) in [
            ("did-web", "0.2.2"),
            ("iota-sdk", "1.1.5"),
            ("oid4vc-manager", "0.1.0"),
            ("oid4vci", "0.1.0"),
            ("oid4vp", "0.1.0"),
        ] {
            assert!(
                locked_package(lockfile, name, version).contains("\"reqwest 0.11.27\""),
                "expected {name} {version} to select reqwest 0.11.27"
            );
        }

        let identity = locked_package(lockfile, "agent_identity", "0.1.0");
        assert!(identity.contains("\"reqwest 0.12.28\""));
        let patched_reqwest = locked_package(lockfile, "reqwest", "0.12.28");
        assert!(patched_reqwest.contains("\"h2 0.4.18\""));
        assert!(patched_reqwest.contains("\"hyper 1.10.1\""));
    }

    fn locked_package<'a>(lockfile: &'a str, name: &str, version: &str) -> &'a str {
        lockfile
            .split("[[package]]")
            .find(|record| {
                record.lines().any(|line| line == format!("name = \"{name}\""))
                    && record.lines().any(|line| line == format!("version = \"{version}\""))
            })
            .unwrap_or_else(|| panic!("missing {name} {version} from Cargo.lock"))
    }

    #[cfg(feature = "allow-localhost")]
    pub mod allow_localhost_tests {
        use super::*;

        #[tokio::test]
        // DISCLAIMER: The DID Configuration specification strictly requires a non-empty `linked_dids` array.
        // This test asserts that the parser's validation error is intentionally swallowed, returning an
        // empty list instead. This is a deliberate bypass to prevent local HTTP testing from failing
        // due to domain linkage requirements. See `docs/adr/0002-allow-localhost-http-fallback-for-local-testing.md`
        // for the full context.
        async fn test_fetch_linked_dids_empty_succeeds_with_fallback() {
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
            let services = IdentityServices::new(subject);

            let issuer_url: Url = mock_server.uri().parse().unwrap();
            let result = services.fetch_linked_dids(&issuer_url).await;

            // When allow-localhost is on, the error is swallowed and fallback is returned.
            assert!(result.unwrap().is_empty());
        }
    }
}
