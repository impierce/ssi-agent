use crate::connection::{
    aggregate::{LinkedCredentialValidation, LinkedVpValidation, ValidationResult},
    error::ConnectionError,
};
use crate::dns::CnameResolver;
use crate::document::aggregate::{Document, Status};
use crate::service::error::ServiceError;
use agent_secret_manager::subject::Subject;
use chrono::{DateTime, Utc};
use identity_kit::{
    did::{Did, DidDocument, VerificationMaterial},
    domain_linkage::{DomainLinkageConfiguration, DomainLinkageValidator},
    jose::{DefaultVerifier, Jwk},
    linked_vp::{self, LinkedVpError, LinkedVpValidationOptions, LinkedVpValidator},
    resolver::CompositeKeyResolver,
    vc::{
        validation::{
            decode_credential, extract_holder, extract_issuer, CredentialValidationOptions, FailFast, KeyReference,
            KeyResolutionError, KeyResolver, KeyRole, StatusPolicy, SubjectHolderRelationship,
        },
        Jwt,
    },
};
use oid4vc_core::utils::jwt::get_unverified_jwt_claims;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use reqwest::{redirect, Client, StatusCode};
use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::lookup_host;
use tracing::{info, warn};
use url::{Host, Url};

const OUTBOUND_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const LINKED_VP_RESPONSE_LIMIT: usize = 5 * 1024 * 1024;
const DID_CONFIGURATION_RESPONSE_LIMIT: usize = 1024 * 1024;

/// Why a domain's DID configuration could not be fetched or parsed.
#[derive(Debug, thiserror::Error)]
pub enum DidConfigurationError {
    /// The domain does not publish a DID configuration (`404`/`410`), so it links no DIDs.
    #[error("{0}")]
    NotFound(String),
    /// The DID configuration could not be retrieved or understood, which says nothing about the
    /// DIDs the domain links.
    #[error("{0}")]
    Unavailable(String),
}

/// A DID extracted from a domain's DID configuration, together with the outcome of its domain
/// linkage verification.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedDid {
    pub did: Did,
    pub domain_linkage_valid: bool,
    pub domain_linkage_error: Option<String>,
}

/// Where the signed presentations that can be published as Linked Verifiable Presentations come from.
#[async_trait::async_trait]
pub trait LinkedVerifiablePresentationSource: Send + Sync {
    async fn signed_presentation(&self, presentation_id: &str) -> anyhow::Result<Option<Jwt>>;
}

/// Offers no presentations, for deployments and tests that do not publish any.
pub struct NoLinkedVerifiablePresentations;

#[async_trait::async_trait]
impl LinkedVerifiablePresentationSource for NoLinkedVerifiablePresentations {
    async fn signed_presentation(&self, _presentation_id: &str) -> anyhow::Result<Option<Jwt>> {
        Ok(None)
    }
}

impl LinkedDid {
    /// A DID whose domain linkage could not be established.
    pub fn unverified(did: Did, error: impl Into<String>) -> Self {
        Self {
            did,
            domain_linkage_valid: false,
            domain_linkage_error: Some(error.into()),
        }
    }
}

pub(crate) fn document_can_link(document: &Document) -> bool {
    document.document.is_some()
        && document.status != Status::Disabled
        && document.did_method.is_some_and(|method| method.supports_update())
}

/// Resolves keys from a DID document the caller already holds. A key of another DID is unsupported, which lets a
/// [`CompositeKeyResolver`] try the next resolver.
struct DocumentKeys(DidDocument);

#[async_trait::async_trait]
impl KeyResolver for DocumentKeys {
    async fn resolve_key(&self, key: &KeyReference, role: KeyRole) -> Result<Jwk, KeyResolutionError> {
        let KeyReference::DidUrl(id) = key else {
            return Err(KeyResolutionError::unsupported(key));
        };
        if id.did() != &self.0.id {
            return Err(KeyResolutionError::unsupported(key));
        }
        let method = self
            .0
            .resolve_verification_method(id, Some(role.relationship()))
            .map_err(KeyResolutionError::new)?;
        match &method.material {
            VerificationMaterial::PublicKeyJwk(jwk) => Ok(jwk.public()),
            VerificationMaterial::PublicKeyMultibase(multikey) => {
                Jwk::from_multikey(multikey).map_err(KeyResolutionError::new)
            }
            _ => Err(KeyResolutionError::msg(format!(
                "verification method `{id}` has no supported key material"
            ))),
        }
    }
}

/// Checks that a presentation can be published as a Linked Verifiable Presentation and returns its
/// holder DID. The holder must own one of the `linkable_documents`, have signed the presentation,
/// and be the subject of every credential in it; a presentation without credentials is rejected.
pub(crate) async fn validate_linked_verifiable_presentation(
    presentation_id: &str,
    presentation: Option<&Jwt>,
    linkable_documents: &[DidDocument],
) -> Result<String, ServiceError> {
    let invalid = |reason: String| ServiceError::PresentationInvalid(presentation_id.to_owned(), reason);
    let presentation = presentation.ok_or_else(|| ServiceError::PresentationNotFound(presentation_id.to_owned()))?;
    let holder = extract_holder(presentation).map_err(|error| invalid(error.to_string()))?;
    let holder_document = linkable_documents
        .iter()
        .find(|document| document.id == holder)
        .ok_or_else(|| {
            invalid(format!(
                "holder DID `{holder}` has no enabled document that can publish services"
            ))
        })?;
    // Only the holder's own signature is checked: the embedded credentials are signed by their issuers, whose keys
    // the holder's document doesn't hold, so their errors are not the presentation's.
    let options = LinkedVpValidationOptions::new().with_subject_holder(SubjectHolderRelationship::Any);
    match LinkedVpValidator::new(DocumentKeys(holder_document.clone()), DefaultVerifier)
        .validate(presentation, &holder, &options)
        .await
    {
        Ok(_) => {}
        Err(LinkedVpError::Presentation(error)) if error.presentation.is_none() => {}
        Err(error) => return Err(invalid(error.to_string())),
    }
    let credentials = linked_vp::decode(presentation)
        .map_err(|error| invalid(error.to_string()))?
        .presentation
        .verifiable_credential;
    if credentials.is_empty() {
        return Err(invalid("the presentation contains no credentials".to_owned()));
    }
    if credentials.iter().any(|credential_jwt| {
        decode_credential(credential_jwt).map_or(true, |decoded| {
            !decoded
                .credential
                .credential_subject
                .iter()
                .any(|subject| subject.id.as_ref().is_some_and(|id| id.as_str() == holder.as_str()))
        })
    }) {
        return Err(invalid(format!(
            "holder DID `{holder}` is not the subject of every embedded credential"
        )));
    }
    Ok(holder.to_string())
}

/// Identity services.
pub struct IdentityServices {
    pub subject: Arc<Subject>,
    pub public_url: Url,
    pub linkage_clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    /// Resolves a linked domain's `CNAME` chain when verifying that it points at this deployment.
    pub cname_resolver: Arc<dyn CnameResolver>,
    pub client: Client,
    /// Whether outbound identity resources on the local network may use `http`. Only `true` in
    /// local development builds; public HTTP and sensitive address ranges remain blocked. See
    /// `docs/adr/0002-allow-localhost-http-fallback-for-local-testing.md`.
    pub allow_local_network_outbound: bool,
    pub linked_verifiable_presentations: Arc<dyn LinkedVerifiablePresentationSource>,
}

impl IdentityServices {
    pub fn new(subject: Arc<Subject>, public_url: Url) -> Self {
        Self {
            subject,
            public_url,
            linkage_clock: Arc::new(Utc::now),
            cname_resolver: crate::dns::system_resolver(),
            client: Client::new(),
            allow_local_network_outbound: cfg!(feature = "allow-localhost"),
            linked_verifiable_presentations: Arc::new(NoLinkedVerifiablePresentations),
        }
    }

    #[cfg(feature = "test_utils")]
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        let subject = futures::executor::block_on(async { Subject::new().await }).with_local_network_resolver();

        // Tests drive outbound identity resources from mock servers on `127.0.0.1`, which the
        // production policy rejects.
        Arc::new(Self {
            allow_local_network_outbound: true,
            linkage_clock: Arc::new(crate::service::aggregate::test_utils::issuance_date),
            ..Self::new(Arc::new(subject), "https://my-domain.example.org/".parse().unwrap())
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

    /// Fetches the domain's DID configuration and validates each linked DID against it.
    ///
    /// Returns an error when the DID configuration cannot be fetched or parsed, e.g. because its
    /// owner has not linked the domain, so callers can tell that apart from a configuration whose
    /// linked DIDs fail validation.
    pub async fn fetch_linked_dids(&self, url: &Url) -> Result<Vec<LinkedDid>, DidConfigurationError> {
        let config = self.fetch_domain_linkage_configuration(url).await?;
        let linked_dids = extract_linked_dids(&config);

        if linked_dids.is_empty() {
            info!("No linked DIDs found in configuration");
            return Ok(vec![]);
        }

        // The subject resolves the key of each DID, including the DID methods IdentityKit doesn't.
        let validator = DomainLinkageValidator::new(Arc::clone(&self.subject), DefaultVerifier);
        let options = CredentialValidationOptions::new();
        let mut results = Vec::with_capacity(linked_dids.len());

        // Linkage is tracked per DID rather than aggregated into a single boolean: a single failing
        // DID must not make the other linked DIDs of the same domain indistinguishable from it.
        for did in linked_dids {
            let result = match validator.validate_linkage(&config, &did, url, &options).await {
                Ok(_) => {
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
            let document = match self.subject.resolve_document(did).await {
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
        holder_document: &DidDocument,
        url: Url,
    ) -> LinkedVpValidation {
        let last_validated_at = self.now();
        let did = linked_did.did.to_string();

        let (valid, error, credentials) = match self.validated_credentials(holder_document, &url).await {
            Ok((credentials, presentation_error)) => {
                let invalid = credentials.iter().filter(|c| !c.result.valid).count();
                let error = presentation_error.or_else(|| {
                    (invalid > 0).then(|| {
                        format!(
                            "{invalid} of {} embedded credentials failed validation",
                            credentials.len()
                        )
                    })
                });
                (error.is_none(), error, credentials)
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

    /// The embedded credentials, each with its own outcome, and the presentation's own error when
    /// its signature could not be verified.
    ///
    /// A presentation that fails to verify does not invalidate the credentials inside it: each one
    /// carries its issuer's signature, checked separately below against that issuer's DID Document.
    /// Reporting them flagged rather than discarding them is what [`LinkedVpValidation`] documents,
    /// and it keeps what a holder publishes legible — plainly marked unverified — while a signing
    /// fault is fixed. What is lost with the presentation is the proof that *this holder* assembled
    /// this set, so nothing here may be shown as the holder's own without that caveat.
    async fn validated_credentials(
        &self,
        holder_document: &DidDocument,
        url: &Url,
    ) -> anyhow::Result<(Vec<LinkedCredentialValidation>, Option<String>)> {
        let presentation_jwt: Jwt = self.fetch_linked_verifiable_presentation(url).await?.parse()?;
        // The holder's key comes from the document already resolved; the issuers' keys are resolved by the subject.
        let resolver = CompositeKeyResolver::new()
            .with(DocumentKeys(holder_document.clone()))
            .with_shared(self.subject.clone());
        let validator = LinkedVpValidator::new(resolver, DefaultVerifier);
        let mut credential_options = CredentialValidationOptions::new();
        credential_options.status = StatusPolicy::SkipUnsupported;
        let options = LinkedVpValidationOptions::new()
            .with_credentials(credential_options.clone())
            .with_subject_holder(SubjectHolderRelationship::Any)
            .with_fail_fast(FailFast::AllErrors);

        // What the presentation carries, decoded without verifying anything, so that credentials can be reported
        // whatever their outcome.
        let credential_jwts = linked_vp::decode(&presentation_jwt)
            .map(|decoded| decoded.presentation.verifiable_credential)
            .unwrap_or_default();
        let last_validated_at = self.now();
        let result = |credential_jwt: &str, error: Option<String>| LinkedCredentialValidation {
            credential: credential_jwt
                .parse()
                .ok()
                .and_then(|jwt| decode_credential(&jwt).ok())
                .map(|decoded| decoded.credential),
            jwt: credential_jwt.to_owned(),
            result: ValidationResult {
                valid: error.is_none(),
                error,
                last_validated_at,
            },
        };

        // Every embedded credential is reported with its own outcome. A credential that fails to
        // validate neither disappears from the response nor marks the others as untrustworthy; the
        // caller derives the presentation-level verdict from these per-credential results.
        match validator
            .validate(&presentation_jwt, &holder_document.id, &options)
            .await
        {
            Ok(_) => Ok((
                credential_jwts.iter().map(|jwt| result(jwt.as_str(), None)).collect(),
                None,
            )),
            Err(LinkedVpError::Presentation(error)) if error.presentation.is_none() => Ok((
                credential_jwts
                    .iter()
                    .enumerate()
                    .map(|(index, jwt)| {
                        result(
                            jwt.as_str(),
                            error.credentials.get(&index).map(|error| credential_error(jwt, error)),
                        )
                    })
                    .collect(),
                None,
            )),
            Err(error) => {
                warn!("Failed to validate the linked presentation at '{url}': {error}");
                // The credentials weren't validated with the presentation, so each one is validated on its own. They
                // are read one by one, so that a malformed entry, which fails decoding the presentation as a whole,
                // doesn't hide the others.
                let credential_validator = validator.presentation_validator().credential_validator();
                let presented = presented_credentials(&presentation_jwt);
                let mut credentials = Vec::with_capacity(presented.len());
                for credential_jwt in &presented {
                    let error = match credential_jwt.parse::<Jwt>() {
                        Ok(jwt) => credential_validator
                            .validate(&jwt, &credential_options)
                            .await
                            .err()
                            .map(|error| credential_error(&jwt, &error)),
                        Err(error) => {
                            warn!("Failed to decode a linked credential: {error}");
                            Some(format!("Failed to decode credential: {error}"))
                        }
                    };
                    credentials.push(result(credential_jwt, error));
                }
                Ok((credentials, Some(error.to_string())))
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
        let addresses = resolve_outbound_url(url, self.allow_local_network_outbound)
            .await
            .map_err(|error| anyhow::anyhow!("Refused to fetch linked VP from '{url}': {error}"))?;
        let client = pinned_outbound_client(url, &addresses)?;

        let response = client.get(url.as_str()).send().await?;
        if response.status().is_redirection() {
            anyhow::bail!(
                "Linked VP endpoint '{url}' responded with a redirect, which is not followed for security reasons"
            );
        }

        let body = read_limited_response(response.error_for_status()?, LINKED_VP_RESPONSE_LIMIT, "Linked VP").await?;
        String::from_utf8(body).map_err(|error| anyhow::anyhow!("Linked VP response is not valid UTF-8: {error}"))
    }

    async fn fetch_domain_linkage_configuration(
        &self,
        url: &Url,
    ) -> Result<DomainLinkageConfiguration, DidConfigurationError> {
        let mut url = url.clone();
        url.set_path("/.well-known/did-configuration.json");

        info!("Fetching DID configuration from: {url}");

        let addresses = resolve_outbound_url(&url, self.allow_local_network_outbound)
            .await
            .map_err(|error| {
                DidConfigurationError::Unavailable(format!("Refused to fetch DID configuration from '{url}': {error}"))
            })?;
        let client = pinned_outbound_client(&url, &addresses)
            .map_err(|error| DidConfigurationError::Unavailable(error.to_string()))?;
        let response = client
            .get(url.as_str())
            .send()
            .await
            .map_err(|error| DidConfigurationError::Unavailable(error.to_string()))?;
        if response.status().is_redirection() {
            return Err(DidConfigurationError::Unavailable(format!(
                "DID configuration endpoint '{url}' responded with a redirect, which is not followed for security reasons"
            )));
        }
        let response = response.error_for_status().map_err(|error| match error.status() {
            Some(StatusCode::NOT_FOUND | StatusCode::GONE) => DidConfigurationError::NotFound(error.to_string()),
            _ => DidConfigurationError::Unavailable(error.to_string()),
        })?;
        let response = read_limited_response(response, DID_CONFIGURATION_RESPONSE_LIMIT, "DID configuration")
            .await
            .map_err(|error| DidConfigurationError::Unavailable(error.to_string()))?;
        // Linked Data Proof entries are carried along and not validated.
        let config: DomainLinkageConfiguration = serde_json::from_slice(&response).map_err(|error| {
            DidConfigurationError::Unavailable(
                format!("failed to deserialize DomainLinkageConfiguration from JSON: {error}"),
                // TODO: Add more detailed error info.
            )
        })?;
        // A configuration without entries is malformed rather than one that links no DIDs, so a sync keeps the DIDs
        // it knows.
        if config.linked_dids.is_empty() {
            return Err(DidConfigurationError::Unavailable(
                "the DID configuration contains no linked DIDs".to_string(),
            ));
        }
        Ok(config)
    }
}

/// Why a linked credential failed validation, naming its issuer as the credential claims it.
/// The entries of the presentation's `vp.verifiableCredential`, one credential or a list of them, read without
/// decoding or verifying anything else.
fn presented_credentials(presentation_jwt: &Jwt) -> Vec<String> {
    let Ok(claims) = presentation_jwt
        .decode()
        .and_then(|jws| jws.claims::<serde_json::Value>())
    else {
        return Vec::new();
    };
    match claims.pointer("/vp/verifiableCredential") {
        Some(serde_json::Value::String(jwt)) => vec![jwt.clone()],
        Some(serde_json::Value::Array(jwts)) => jwts
            .iter()
            .filter_map(|jwt| jwt.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn credential_error(credential_jwt: &Jwt, error: &impl std::fmt::Display) -> String {
    match extract_issuer(credential_jwt) {
        Ok(issuer) => {
            warn!("Failed to validate linked credential issued by {issuer}: {error}");
            format!("Failed to validate credential issued by '{issuer}': {error}")
        }
        Err(extract_error) => {
            warn!("Failed to extract linked credential issuer: {extract_error}");
            format!("Failed to extract credential issuer: {extract_error}")
        }
    }
}

/// Extracts the subject DID of every linked-DID JWT in a domain linkage configuration, without
/// verifying their signatures. Used both to discover DIDs to validate and, for self-verification,
/// to compare a fetched configuration against the DIDs UniCore expects to have published.
///
/// Deduplicated: a configuration linking one DID to several origins carries one credential per
/// origin, and every caller here is interested in the distinct DIDs rather than in the credentials.
pub(crate) fn extract_linked_dids(config: &DomainLinkageConfiguration) -> Vec<Did> {
    let mut dids: Vec<Did> = Vec::new();
    for did in config.jwts().filter_map(|(_, jwt)| linked_did_subject(jwt)) {
        if !dids.contains(&did) {
            dids.push(did);
        }
    }
    dids
}

/// Groups the DIDs a domain linkage configuration links by the origin each credential claims.
///
/// Each Domain Linkage Credential claims exactly one origin, so this is what says which DIDs UniCore
/// published *for a given origin* — the unit a verifier of that origin actually checks.
pub(crate) fn linked_dids_by_origin(config: &DomainLinkageConfiguration) -> HashMap<Url, BTreeSet<Did>> {
    let mut by_origin: HashMap<Url, BTreeSet<Did>> = HashMap::new();
    for (_, linkage) in config.entries() {
        let Ok(linkage) = linkage else { continue };
        let Ok(origin) = linkage.origin.parse() else { continue };
        by_origin.entry(origin).or_default().insert(linkage.did);
    }
    by_origin
}

/// The subject DID a linked-DID JWT names, read **without verifying its signature**.
fn linked_did_subject(jwt: &Jwt) -> Option<Did> {
    let claims = get_unverified_jwt_claims(&serde_json::json!(jwt.as_str())).ok()?;
    claims
        .get("sub")
        .or_else(|| claims.get("iss"))
        .and_then(|value| value.as_str())?
        .parse()
        .ok()
}

/// Resolves and vets every address that may be used for an outbound identity-resource request.
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
fn pinned_outbound_client(url: &Url, addresses: &[SocketAddr]) -> anyhow::Result<Client> {
    let mut builder = Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(OUTBOUND_REQUEST_TIMEOUT)
        // A configured proxy would resolve the target independently and bypass the pinned result.
        .no_proxy();

    if let Some(Host::Domain(host)) = url.host() {
        builder = builder.resolve_to_addrs(host, addresses);
    }

    builder.build().map_err(Into::into)
}

async fn read_limited_response(
    mut response: reqwest::Response,
    limit: usize,
    resource: &str,
) -> anyhow::Result<Vec<u8>> {
    if response.content_length().is_some_and(|length| length > limit as u64) {
        anyhow::bail!("{resource} response exceeds the {limit}-byte limit");
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("{resource} response exceeds the {limit}-byte limit");
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
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

/// The URLs of the document's linked VP services. Each URL is read on its own, so that a malformed one doesn't
/// hide the others of the same service.
fn linked_verifiable_presentation_urls(document: &DidDocument) -> Vec<Url> {
    document
        .service
        .iter()
        .filter(|service| service.has_type(linked_vp::SERVICE_TYPE))
        .flat_map(|service| service.service_endpoint.uris())
        .filter_map(|uri| {
            Url::parse(uri.as_str())
                .inspect_err(|error| warn!("Failed to parse linked VP URL '{uri}': {error}"))
                .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_kit::{
        did::{DidUrl, MethodType, VerificationMaterial, VerificationMethod, VerificationRelationship},
        jose::{JwsAlgorithm, JwsHeader, KeyType},
        keys::{InMemoryKeyStore, KeyId, KeySigner, KeySpec, KeyStore},
        linked_vp::LinkedPresentationBuilder,
        vc::{Credential, Subject as CredentialSubject},
    };
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const LINKED_DID_JWT: &str = "eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNI3o2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSJ9.eyJleHAiOjE3NjQ4NzkxMzksImlzcyI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNIiwibmJmIjoxNjA3MTEyNzM5LCJzdWIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9pZGVudGl0eS5mb3VuZGF0aW9uLy53ZWxsLWtub3duL2RpZC1jb25maWd1cmF0aW9uL3YxIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rb1RIc2dOTnJieThKekNOUTFpUkx5VzVRUTZSOFh1dTZBQThpZ0dyTVZQVU0iLCJvcmlnaW4iOiJpZGVudGl0eS5mb3VuZGF0aW9uIn0sImV4cGlyYXRpb25EYXRlIjoiMjAyNS0xMi0wNFQxNDoxMjoxOS0wNjowMCIsImlzc3VhbmNlRGF0ZSI6IjIwMjAtMTItMDRUMTQ6MTI6MTktMDY6MDAiLCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJEb21haW5MaW5rYWdlQ3JlZGVudGlhbCJdfX0.aUFNReA4R5rcX_oYm3sPXqWtso_gjPHnWZsB6pWcGv6m3K8-4JIAvFov3ZTM8HxPOrOL17Qf4vBFdY9oK0HeCQ";
    const TEST_DID: &str = "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM";

    /// A `did:web` holder served by a mock server, whose keys sign its credentials and presentations.
    struct Holder {
        document: DidDocument,
        keys: Arc<dyn KeyStore>,
    }

    impl Holder {
        fn new(mock_server: &MockServer) -> Self {
            let did: Did = format!("did:web:localhost%3A{}", mock_server.address().port())
                .parse()
                .unwrap();
            Self {
                document: DidDocument::new(did),
                keys: Arc::new(InMemoryKeyStore::new()),
            }
        }

        fn did(&self) -> &str {
            self.document.id.as_str()
        }

        /// Generates an Ed25519 key and adds it to the document under every relationship, with its key id as the
        /// fragment.
        async fn add_key(&mut self) -> DidUrl {
            self.add_key_as(MethodType::JSON_WEB_KEY_2020, VerificationMaterial::jwk)
                .await
        }

        /// As [`Self::add_key`], with the key as `publicKeyMultibase`.
        async fn add_multikey(&mut self) -> DidUrl {
            self.add_key_as(MethodType::MULTIKEY, |jwk| {
                VerificationMaterial::multibase(jwk.to_multikey().unwrap())
            })
            .await
        }

        async fn add_key_as(
            &mut self,
            method_type: MethodType,
            material: impl FnOnce(&Jwk) -> VerificationMaterial,
        ) -> DidUrl {
            let key = self
                .keys
                .generate(KeySpec::new(KeyType::Ed25519, JwsAlgorithm::EdDSA))
                .await
                .unwrap();
            let id = self.document.id.join(&format!("#{}", key.as_str())).unwrap();
            let jwk = self.keys.public_jwk(&key).await.unwrap();
            let method = VerificationMethod::new(id.clone(), method_type, self.document.id.clone(), material(&jwk));
            self.document
                .insert_verification_method(method, VerificationRelationship::ALL.iter().copied())
                .unwrap();
            id
        }

        async fn signer(&self, method: &DidUrl) -> KeySigner<Arc<dyn KeyStore>> {
            let key = KeyId::new(method.fragment().unwrap());
            KeySigner::new(Arc::clone(&self.keys), key, JwsAlgorithm::EdDSA)
                .await
                .unwrap()
        }

        fn credential(&self) -> Credential {
            let subject =
                CredentialSubject::new(self.document.id.clone()).with_property("Content", "Awesome collaboration!");
            Credential::new(
                self.document.id.clone(),
                "2026-03-04T12:00:00Z".parse().unwrap(),
                subject,
            )
            .with_type("Endorsement")
        }

        async fn credential_jwt(&self, credential: &Credential, method: &DidUrl) -> Jwt {
            let header = JwsHeader::new(JwsAlgorithm::EdDSA)
                .with_typ("JWT")
                .with_kid(method.as_str());
            Jwt::sign(
                &header,
                &credential.to_jwt_claims().unwrap(),
                &self.signer(method).await,
            )
            .await
            .unwrap()
        }

        async fn presentation_jwt(&self, credentials: Vec<Jwt>, method: &DidUrl) -> Jwt {
            credentials
                .into_iter()
                .fold(
                    LinkedPresentationBuilder::new(self.document.id.clone()),
                    LinkedPresentationBuilder::with_credential,
                )
                .sign(method, &self.signer(method).await)
                .await
                .unwrap()
        }

        /// Serves the document, linking `presentation_jwt`, and the presentation.
        async fn serve(mut self, mock_server: &MockServer, presentation_jwt: &Jwt) {
            let presentation_url: Url = format!("{}/linked-vp", mock_server.uri()).parse().unwrap();
            let service_id = self
                .document
                .id
                .join("#linked-verifiable-presentation-service")
                .unwrap();
            self.document
                .insert_service(linked_vp::service(service_id, [presentation_url]).unwrap())
                .unwrap();

            self.serve_document(mock_server).await;
            Mock::given(method("GET"))
                .and(path("/linked-vp"))
                .respond_with(ResponseTemplate::new(200).set_body_string(presentation_jwt.as_str()))
                .mount(mock_server)
                .await;
        }

        /// Serves the document alone, as an issuer that links no presentation does.
        async fn serve_document(&self, mock_server: &MockServer) {
            Mock::given(method("GET"))
                .and(path("/.well-known/did.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(&self.document))
                .mount(mock_server)
                .await;
        }
    }

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
        let mut holder = Holder::new(&mock_server);
        let holder_did = holder.did().to_owned();
        let method = holder.add_key().await;
        let credential = holder.credential();
        let credential_jwt = holder.credential_jwt(&credential, &method).await;
        let presentation_jwt = holder.presentation_jwt(vec![credential_jwt], &method).await;
        holder.serve(&mock_server, &presentation_jwt).await;

        let services = IdentityServices::default();
        let resolved_document = services
            .subject
            .resolve_document(&holder_did.parse().unwrap())
            .await
            .unwrap();
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
    async fn fetch_linked_vp_validations_resolves_multibase_holder_keys() {
        let mock_server = MockServer::start().await;
        let mut holder = Holder::new(&mock_server);
        let holder_did = holder.did().to_owned();
        let method = holder.add_multikey().await;
        let credential_jwt = holder.credential_jwt(&holder.credential(), &method).await;
        let presentation_jwt = holder.presentation_jwt(vec![credential_jwt], &method).await;
        holder.serve(&mock_server, &presentation_jwt).await;

        let validations = IdentityServices::default()
            .fetch_linked_vp_validations(&[verified_linked_did(&holder_did)])
            .await;

        assert_eq!(validations.len(), 1);
        assert!(validations[0].result.valid, "{:?}", validations[0].result.error);
        assert!(validations[0].credentials[0].result.valid);
    }

    #[tokio::test]
    async fn document_keys_resolve_only_under_the_role_relationship() {
        let keys = InMemoryKeyStore::new();
        let key = keys
            .generate(KeySpec::new(KeyType::Ed25519, JwsAlgorithm::EdDSA))
            .await
            .unwrap();
        let jwk = keys.public_jwk(&key).await.unwrap();
        let mut document = DidDocument::new("did:web:example.org".parse().unwrap());
        let id = document.id.join("#key-1").unwrap();
        let method = VerificationMethod::new(
            id.clone(),
            MethodType::JSON_WEB_KEY_2020,
            document.id.clone(),
            VerificationMaterial::jwk(&jwk),
        );
        document
            .insert_verification_method(method, [VerificationRelationship::Authentication])
            .unwrap();
        let document_keys = DocumentKeys(document);
        let reference = KeyReference::DidUrl(id);

        assert_eq!(
            document_keys.resolve_key(&reference, KeyRole::Subject).await.unwrap(),
            jwk.public()
        );
        // Not unsupported, so that a `CompositeKeyResolver` doesn't try the next resolver.
        let error = document_keys
            .resolve_key(&reference, KeyRole::Issuer)
            .await
            .unwrap_err();
        assert!(!error.is_unsupported());

        let other_did = KeyReference::DidUrl("did:web:example.com#key-1".parse().unwrap());
        assert!(document_keys
            .resolve_key(&other_did, KeyRole::Subject)
            .await
            .unwrap_err()
            .is_unsupported());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_resolves_the_keys_of_other_issuers() {
        let holder_server = MockServer::start().await;
        let issuer_server = MockServer::start().await;
        let mut holder = Holder::new(&holder_server);
        let mut issuer = Holder::new(&issuer_server);
        let holder_did = holder.did().to_owned();
        let holder_method = holder.add_key().await;
        let issuer_method = issuer.add_key().await;

        // Issued to the holder by another DID, whose key is not in the holder's document.
        let credential = Credential::new(
            issuer.document.id.clone(),
            "2026-03-04T12:00:00Z".parse().unwrap(),
            CredentialSubject::new(holder.document.id.clone()).with_property("Content", "Great collab!"),
        )
        .with_type("Endorsement");
        let credential_jwt = issuer.credential_jwt(&credential, &issuer_method).await;
        let presentation_jwt = holder.presentation_jwt(vec![credential_jwt], &holder_method).await;
        issuer.serve_document(&issuer_server).await;
        holder.serve(&holder_server, &presentation_jwt).await;

        let validations = IdentityServices::default()
            .fetch_linked_vp_validations(&[verified_linked_did(&holder_did)])
            .await;

        assert_eq!(validations.len(), 1);
        assert!(validations[0].result.valid, "{:?}", validations[0].result.error);
        assert_eq!(validations[0].credentials.len(), 1);
        assert!(validations[0].credentials[0].result.valid);
        assert_eq!(validations[0].credentials[0].credential, Some(credential));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_reports_credentials_next_to_a_malformed_entry() {
        let mock_server = MockServer::start().await;
        let mut holder = Holder::new(&mock_server);
        let holder_did = holder.did().to_owned();
        let method = holder.add_key().await;
        let credential = holder.credential();
        let credential_jwt = holder.credential_jwt(&credential, &method).await;

        // A presentation whose second entry is not a JWT and whose third is not even a string, which fail decoding the
        // presentation as a whole.
        let mut claims = LinkedPresentationBuilder::new(holder.document.id.clone())
            .with_credential(credential_jwt.clone())
            .jwt_claims()
            .unwrap();
        let entries = claims.additional["vp"]["verifiableCredential"].as_array_mut().unwrap();
        entries.push("not-a-jwt".into());
        entries.push(42.into());
        let header = JwsHeader::new(JwsAlgorithm::EdDSA)
            .with_typ("JWT")
            .with_kid(method.as_str());
        let presentation_jwt = Jwt::sign(&header, &claims, &holder.signer(&method).await)
            .await
            .unwrap();
        holder.serve(&mock_server, &presentation_jwt).await;

        let validations = IdentityServices::default()
            .fetch_linked_vp_validations(&[verified_linked_did(&holder_did)])
            .await;

        assert_eq!(validations.len(), 1);
        assert!(!validations[0].result.valid);
        let credentials = &validations[0].credentials;
        assert_eq!(credentials.len(), 2);
        assert_eq!(credentials[0].jwt, credential_jwt.as_str());
        assert!(credentials[0].result.valid);
        assert_eq!(credentials[0].credential, Some(credential));
        assert_eq!(credentials[1].jwt, "not-a-jwt");
        assert!(!credentials[1].result.valid);
        assert!(credentials[1].credential.is_none());
    }

    #[test]
    fn linked_verifiable_presentation_urls_skips_only_malformed_urls() {
        let document: DidDocument = serde_json::from_value(serde_json::json!({
            "id": "did:web:example.org",
            "service": [{
                "id": "did:web:example.org#linked-verifiable-presentation-service",
                "type": "LinkedVerifiablePresentation",
                "serviceEndpoint": ["https://example.org/vp-1", "https://example.org:99999/vp-2"]
            }]
        }))
        .unwrap();

        assert_eq!(
            linked_verifiable_presentation_urls(&document),
            vec!["https://example.org/vp-1".parse::<Url>().unwrap()]
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_reports_each_embedded_credential_separately() {
        let mock_server = MockServer::start().await;
        let mut holder = Holder::new(&mock_server);
        let holder_did = holder.did().to_owned();
        let method = holder.add_key().await;
        let credential = holder.credential();
        let credential_jwt = holder.credential_jwt(&credential, &method).await;

        // Same credential with a corrupted signature: the issuer still resolves, but the signature
        // no longer verifies.
        let tampered_jwt: Jwt = {
            let (payload, signature) = credential_jwt.as_str().rsplit_once('.').unwrap();
            let mut signature: Vec<char> = signature.chars().collect();
            signature[0] = if signature[0] == 'A' { 'B' } else { 'A' };
            format!("{payload}.{}", signature.into_iter().collect::<String>())
                .parse()
                .unwrap()
        };

        let presentation_jwt = holder
            .presentation_jwt(vec![credential_jwt, tampered_jwt.clone()], &method)
            .await;
        holder.serve(&mock_server, &presentation_jwt).await;

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

    /// The failure a stale `kid` produces: the presentation is well-formed and its credentials are
    /// genuine, but its signing key is not in the holder's DID Document, so the presentation cannot
    /// be verified. What it carries must still be reported, flagged, rather than vanish.
    #[tokio::test]
    #[serial_test::serial]
    async fn fetch_linked_vp_validations_reports_credentials_of_an_unverifiable_presentation() {
        let mock_server = MockServer::start().await;
        let mut holder = Holder::new(&mock_server);
        let holder_did = holder.did().to_owned();
        let issuing_method = holder.add_key().await;
        let presenting_method = holder.add_key().await;
        let credential = holder.credential();
        let credential_jwt = holder.credential_jwt(&credential, &issuing_method).await;
        let presentation_jwt = holder.presentation_jwt(vec![credential_jwt], &presenting_method).await;

        // The document served to the world never carries the key the presentation was signed with,
        // exactly as a `kid` naming a method that was never published behaves.
        assert!(holder.document.remove_verification_method(&presenting_method).is_some());
        holder.serve(&mock_server, &presentation_jwt).await;

        let services = IdentityServices::default();
        let validations = services
            .fetch_linked_vp_validations(&[verified_linked_did(&holder_did)])
            .await;

        assert_eq!(validations.len(), 1);
        // The presentation is unverifiable, and says so rather than blaming its credentials.
        assert!(!validations[0].result.valid);
        assert!(validations[0].result.error.is_some());
        // The credential inside it is intact, independently verified against its issuer.
        assert_eq!(validations[0].credentials.len(), 1);
        assert!(validations[0].credentials[0].result.valid);
        assert_eq!(validations[0].credentials[0].credential, Some(credential));
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
        let mut services = IdentityServices::new(subject, mock_server.uri().parse().unwrap());
        services.allow_local_network_outbound = true;

        let issuer_url: Url = mock_server.uri().parse().unwrap();
        let dids = services.fetch_linked_dids(&issuer_url).await.unwrap();

        assert_eq!(dids.len(), 1);
        assert_eq!(dids[0].did.as_str(), TEST_DID);

        // Validation will fail because the origin in the JWT is "identity.foundation" and we are fetching the did from the mockserver.
        assert!(!dids[0].domain_linkage_valid);
        assert!(dids[0].domain_linkage_error.is_some());
    }

    #[tokio::test]
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
        let mut services = IdentityServices::new(subject, mock_server.uri().parse().unwrap());
        services.allow_local_network_outbound = true;

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
    async fn domain_linkage_fetch_rejects_local_destinations_by_default() {
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
        let mut services = IdentityServices::new(subject, mock_server.uri().parse().unwrap());
        services.allow_local_network_outbound = false;
        let origin: Url = mock_server.uri().replace("127.0.0.1", "localhost").parse().unwrap();

        let error = services.fetch_domain_linkage_configuration(&origin).await.unwrap_err();

        assert!(matches!(
            error,
            DidConfigurationError::Unavailable(message)
                if message.contains("Refused to fetch DID configuration")
        ));
        assert!(mock_server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn domain_linkage_redirects_are_not_followed() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/did-configuration.json"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/redirected"))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/redirected"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@context": "https://identity.foundation/.well-known/did-configuration/v1",
                "linked_dids": []
            })))
            .mount(&mock_server)
            .await;

        let subject = Arc::new(Subject::new().await);
        let mut services = IdentityServices::new(subject, mock_server.uri().parse().unwrap());
        services.allow_local_network_outbound = true;
        let origin: Url = mock_server.uri().parse().unwrap();

        let error = services.fetch_domain_linkage_configuration(&origin).await.unwrap_err();

        assert!(matches!(
            error,
            DidConfigurationError::Unavailable(message)
                if message.contains("responded with a redirect")
        ));
        assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn domain_linkage_response_size_is_limited() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/did-configuration.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'a'; DID_CONFIGURATION_RESPONSE_LIMIT + 1]))
            .mount(&mock_server)
            .await;

        let subject = Arc::new(Subject::new().await);
        let mut services = IdentityServices::new(subject, mock_server.uri().parse().unwrap());
        services.allow_local_network_outbound = true;
        let origin: Url = mock_server.uri().parse().unwrap();

        let error = services.fetch_domain_linkage_configuration(&origin).await.unwrap_err();

        assert!(matches!(
            error,
            DidConfigurationError::Unavailable(message)
                if message.contains("response exceeds")
        ));
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
        let client = pinned_outbound_client(&url, &[*mock_server.address()]).unwrap();
        let response = client.get(url).send().await.unwrap();

        let body = read_limited_response(response, LINKED_VP_RESPONSE_LIMIT, "Linked VP")
            .await
            .unwrap();
        assert_eq!(body, b"presentation");
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

        let vulnerable_hyper = locked_package(lockfile, "hyper", "0.14.32");
        assert!(vulnerable_hyper.contains("\"h2 0.3.27\""));
        assert!(locked_package(lockfile, "aws-smithy-http-client", "1.5.1").contains("\"hyper 0.14.32\""));

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
}
