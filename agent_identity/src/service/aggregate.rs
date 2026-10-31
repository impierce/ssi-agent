use super::{command::ServiceCommand, error::ServiceError, event::ServiceEvent};
use crate::{
    services::{validate_linked_verifiable_presentation, IdentityServices},
    state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID,
};
use chrono::{DateTime, TimeDelta, Utc};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_kit::{
    did::{DidUrl, Service as DocumentService, VerificationMethod},
    domain_linkage::{self, DomainLinkageConfiguration, DomainLinkageCredentialBuilder},
    linked_vp,
};
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use std::{str::FromStr as _, sync::Arc};
use tracing::{debug, info};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceResource {
    LinkedDomains(DomainLinkageConfiguration),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Service {
    #[serde(rename = "id")]
    pub service_id: String,
    pub service: Option<DocumentService>,
    pub presentations: Vec<LinkedVerifiablePresentation>,
    pub resource: Option<ServiceResource>,
    pub is_deleted: bool,
    /// The origins the published Domain Linkage Credentials claim, sorted and deduplicated. These
    /// are whatever the caller linked at runtime and bear no relation to the deployment's own DID.
    pub origins: Vec<Url>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LinkedVerifiablePresentation {
    pub presentation_id: String,
    /// The DID that signed the presentation and is the subject of its credentials.
    pub holder: String,
    /// The absolute URL at which the presentation was published when it was added.
    #[schema(value_type = String)]
    pub url: Url,
}

pub const LINKED_DOMAINS_VALIDITY_DAYS: u32 = 365;
pub const LINKED_DOMAINS_RENEWAL_WINDOW_DAYS: i64 = 30;

impl Service {
    pub fn is_active(&self) -> bool {
        !self.is_deleted && (self.service.is_some() || !self.presentations.is_empty())
    }

    pub fn needs_renewal(&self, now: DateTime<Utc>) -> bool {
        if !self.is_active() {
            return false;
        }
        let Some(ServiceResource::LinkedDomains(configuration)) = &self.resource else {
            return false;
        };
        let threshold = now + TimeDelta::days(LINKED_DOMAINS_RENEWAL_WINDOW_DAYS);
        configuration.linked_dids.is_empty()
            || configuration.entries().any(|(_, linkage)| {
                linkage
                    .ok()
                    .and_then(|linkage| linkage.expires)
                    .is_none_or(|expiration| expiration <= threshold)
            })
    }

    pub fn is_published(&self, presentation_id: &str) -> bool {
        self.presentations
            .iter()
            .any(|presentation| presentation.presentation_id == presentation_id)
    }

    /// Shared by the aggregate and its projection, so both replay the delta identically.
    pub(crate) fn apply_linked_verifiable_presentations_added(
        &mut self,
        service_id: String,
        presentations: Vec<LinkedVerifiablePresentation>,
    ) {
        self.service_id = service_id;
        for presentation in presentations {
            if !self.is_published(&presentation.presentation_id) {
                self.presentations.push(presentation);
            }
        }
        self.is_deleted = false;
    }

    /// Shared by the aggregate and its projection, so both replay the delta identically.
    pub(crate) fn apply_linked_verifiable_presentations_removed(
        &mut self,
        service_id: String,
        presentation_ids: &[String],
    ) {
        self.service_id = service_id;
        self.presentations
            .retain(|presentation| !presentation_ids.contains(&presentation.presentation_id));
        self.is_deleted = self.presentations.is_empty();
    }
}

fn linked_verifiable_presentation_url(public_url: &Url, presentation_id: &str) -> Result<Url, ServiceError> {
    let mut url = public_url.clone();
    url.path_segments_mut()
        .map_err(|_| ServiceError::InvalidUrlError(public_url.to_string()))?
        .pop_if_empty()
        .push("linked-verifiable-presentations")
        .push(presentation_id);
    Ok(url)
}

pub(crate) fn linked_verifiable_presentation_service(
    service_id: &str,
    document_did: &str,
    presentations: &[LinkedVerifiablePresentation],
) -> Result<DocumentService, ServiceError> {
    use ServiceError::*;

    if presentations.is_empty() {
        return Err(EmptyPresentationIds);
    }
    let id =
        DidUrl::parse(&format!("{document_did}#{service_id}")).map_err(|error| InvalidDidError(error.to_string()))?;

    linked_vp::service(id, presentations.iter().map(|presentation| presentation.url.clone()))
        .map_err(|error| ServiceBuilderError(error.to_string()))
}

/// Reduces a URL to its origin: scheme, host and non-default port, with no path.
///
/// Credentials claim origins, so a caller who supplies a URL with a path must end up linking the same
/// domain as one who supplies the bare origin. Canonicalizing on the way in is what keeps the stored
/// set, the credentials' claims and the published endpoint all speaking about the same thing.
fn canonical_origin(url: &Url) -> Result<Url, ServiceError> {
    url.origin()
        .ascii_serialization()
        .parse()
        .map_err(|err: url::ParseError| ServiceError::InvalidUrlError(err.to_string()))
}

/// Canonicalizes, sorts and deduplicates origins so that a given set always produces byte-identical
/// output, regardless of the order or spelling the caller used.
fn normalized(origins: Vec<Url>) -> Result<Vec<Url>, ServiceError> {
    let mut origins = origins.iter().map(canonical_origin).collect::<Result<Vec<_>, _>>()?;
    origins.sort_unstable();
    origins.dedup();
    Ok(origins)
}

/// Builds the `LinkedDomains` service entry for `origins`.
///
/// The DID Configuration specification allows a `serviceEndpoint` to be *either* a bare origin string
/// or an object with an `origins` array, so a single linked domain is written in the simpler string
/// form and only multiple domains use the array.
fn linked_domains_service(service_id: &str, origins: &[Url]) -> Result<DocumentService, ServiceError> {
    if origins.is_empty() {
        return Err(ServiceError::EmptyOriginsError);
    }
    // This service is DID method-agnostic. When added to an enabled DID Document, its placeholder
    // value is replaced with the appropriate DID method-specific identifier.
    let id = DidUrl::parse(&format!("did:place:holder#{service_id}"))
        .map_err(|err| ServiceError::InvalidDidError(err.to_string()))?;

    domain_linkage::service(id, origins.iter().cloned())
        .map_err(|err| ServiceError::ServiceBuilderError(err.to_string()))
}

/// Issues one Domain Linkage Credential per (origin, verification method) pair — each credential
/// claims exactly one origin, so linking N domains with M signing keys publishes N×M credentials —
/// and pairs them with the `LinkedDomains` service entry naming every origin.
///
/// Emitted in `(origin, verification method)` order so that a given set of inputs always produces a
/// byte-identical `did-configuration.json`.
async fn issue_linked_domains(
    services: &Arc<IdentityServices>,
    service_id: &str,
    verification_methods: Vec<VerificationMethod>,
    origins: &[Url],
    issuance_date: DateTime<Utc>,
) -> Result<(DocumentService, ServiceResource), ServiceError> {
    use ServiceError::*;

    let subject = &services.subject;
    let expiration_date = issuance_date
        // TODO: make this configurable
        .checked_add_signed(TimeDelta::days(LINKED_DOMAINS_VALIDITY_DAYS.into()))
        .ok_or(InvalidTimestampError)?;

    let mut configuration = DomainLinkageConfiguration::new();

    for origin in origins {
        for verification_method in &verification_methods {
            let verification_method_id = &verification_method.id;
            let alg = verification_method
                .public_key_jwk()
                .and_then(|jwk| jwk.alg())
                .ok_or_else(|| MissingVerificationMethodAlgorithm(verification_method_id.to_string()))?;
            let algorithm =
                Algorithm::from_str(alg).map_err(|_| UnsupportedVerificationMethodAlgorithm(alg.to_string()))?;

            // TODO: Currently UniCore always uses the same keys for signing regardless of the DID method.
            // Once we implement DID method-specific keys, then we should use the key of the verification
            // method's DID method here.
            let signer = subject
                .issuer_signer(algorithm)
                .await
                .map_err(|err| SigningError(err.to_string()))?;
            let credential = DomainLinkageCredentialBuilder::new(
                verification_method_id.did().clone(),
                origin,
                issuance_date,
                expiration_date,
            )
            .map_err(|err| DomainLinkageCredentialBuilderError(err.to_string()))?
            .sign(verification_method_id, &signer)
            .await
            .map_err(|err| SigningError(err.to_string()))?;

            configuration = configuration.with_credential(credential);
        }
    }

    if configuration.linked_dids.is_empty() {
        return Err(EmptyLinkedDidsError);
    }

    info!("Configuration Resource: {configuration:#}");

    Ok((
        linked_domains_service(service_id, origins)?,
        ServiceResource::LinkedDomains(configuration),
    ))
}

impl Aggregate for Service {
    type Command = ServiceCommand;
    type Event = ServiceEvent;
    type Error = ServiceError;
    type Services = Arc<IdentityServices>;

    const TYPE: &'static str = "service";

    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use ServiceCommand::*;
        use ServiceError::*;
        use ServiceEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
            AddLinkedDomains {
                service_id,
                verification_methods,
                origins,
            } => {
                if origins.is_empty() {
                    return Err(EmptyOriginsError);
                }
                let origins = normalized([self.origins.clone(), origins].concat())?;
                // Linking a domain that is already linked changes nothing, so nothing is emitted.
                if self.is_active() && origins == self.origins {
                    return Ok(());
                }

                let (service, resource) = issue_linked_domains(
                    services,
                    &service_id,
                    verification_methods,
                    &origins,
                    (services.linkage_clock)(),
                )
                .await?;

                Ok(vec![LinkedDomainsAdded {
                    service_id,
                    service,
                    resource,
                    is_deleted: false,
                    origins,
                }])
            }
            RenewLinkedDomainsCredentials {
                service_id,
                verification_methods,
                only_if_expiring,
            } => {
                if !self.is_active() {
                    return Err(NotFound);
                }
                if only_if_expiring && !self.needs_renewal((services.linkage_clock)()) {
                    return Ok(());
                }

                // Renewal re-signs the linked set as it stands; it never changes which domains are
                // linked.
                let origins = self.origins.clone();
                let (service, resource) = issue_linked_domains(
                    services,
                    &service_id,
                    verification_methods,
                    &origins,
                    (services.linkage_clock)(),
                )
                .await?;

                Ok(vec![LinkedDomainsCredentialsRenewed {
                    service_id,
                    service,
                    resource,
                    is_deleted: false,
                    origins,
                }])
            }
            RemoveLinkedDomains { service_id, origins } => {
                if origins.is_empty() {
                    return Err(EmptyOriginsError);
                }
                // Unlinking a domain that is not linked changes nothing, so nothing is emitted.
                if !self.is_active() {
                    return Ok(());
                }
                let removed = normalized(origins)?;
                let remaining: Vec<Url> = self
                    .origins
                    .iter()
                    .filter(|origin| !removed.contains(origin))
                    .cloned()
                    .collect();
                if remaining == self.origins {
                    return Ok(());
                }

                // Unlinking the last domain leaves nothing to publish, so the whole service goes.
                // Otherwise the remaining origins' credentials are still valid and are kept exactly
                // as issued rather than re-signed — which is also why unlinking a domain keeps
                // working after the signing keys have become unavailable.
                let (service, resource) = if remaining.is_empty() {
                    (None, None)
                } else {
                    let Some(ServiceResource::LinkedDomains(configuration)) = &self.resource else {
                        return Err(NotFound);
                    };
                    let mut configuration = configuration.clone();
                    for origin in &removed {
                        configuration
                            .remove_origin(origin)
                            .map_err(|err| InvalidUrlError(err.to_string()))?;
                    }
                    if configuration.linked_dids.is_empty() {
                        return Err(EmptyLinkedDidsError);
                    }
                    (
                        Some(linked_domains_service(&service_id, &remaining)?),
                        Some(ServiceResource::LinkedDomains(configuration)),
                    )
                };

                Ok(vec![LinkedDomainsRemoved {
                    service_id,
                    service,
                    resource,
                    is_deleted: remaining.is_empty(),
                    origins: remaining,
                }])
            }
            AddLinkedVerifiablePresentations {
                presentation_ids,
                signed_presentations,
                linkable_documents,
            } => {
                if presentation_ids.is_empty() {
                    return Err(EmptyPresentationIds);
                }
                let mut added: Vec<LinkedVerifiablePresentation> = vec![];
                for presentation_id in presentation_ids {
                    if self.is_published(&presentation_id)
                        || added
                            .iter()
                            .any(|presentation| presentation.presentation_id == presentation_id)
                    {
                        continue;
                    }
                    let holder = validate_linked_verifiable_presentation(
                        &presentation_id,
                        signed_presentations.get(&presentation_id),
                        &linkable_documents,
                    )
                    .await?;
                    added.push(LinkedVerifiablePresentation {
                        url: linked_verifiable_presentation_url(&services.public_url, &presentation_id)?,
                        presentation_id,
                        holder,
                    });
                }
                if added.is_empty() {
                    return Ok(());
                }
                Ok(vec![LinkedVerifiablePresentationsAdded {
                    service_id: LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
                    presentations: added,
                }])
            }
            RemoveLinkedVerifiablePresentations { presentation_ids } => {
                if presentation_ids.is_empty() {
                    return Err(EmptyPresentationIds);
                }
                let removed: Vec<String> = self
                    .presentations
                    .iter()
                    .map(|presentation| &presentation.presentation_id)
                    .filter(|presentation_id| presentation_ids.contains(presentation_id))
                    .cloned()
                    .collect();
                if removed.is_empty() {
                    return Ok(());
                }
                Ok(vec![LinkedVerifiablePresentationsRemoved {
                    service_id: LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
                    presentation_ids: removed,
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use ServiceEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            LinkedDomainsAdded {
                service_id,
                service,
                resource,
                is_deleted,
                origins,
            }
            | LinkedDomainsCredentialsRenewed {
                service_id,
                service,
                resource,
                is_deleted,
                origins,
            } => {
                self.service_id = service_id;
                self.service.replace(service);
                self.resource.replace(resource);
                self.is_deleted = is_deleted;
                self.origins = origins;
            }
            LinkedDomainsRemoved {
                service_id,
                service,
                resource,
                is_deleted,
                origins,
            } => {
                self.service_id = service_id;
                self.service = service;
                self.resource = resource;
                self.is_deleted = is_deleted;
                self.origins = origins;
            }
            LinkedVerifiablePresentationsAdded {
                service_id,
                presentations,
            } => self.apply_linked_verifiable_presentations_added(service_id, presentations),
            LinkedVerifiablePresentationsRemoved {
                service_id,
                presentation_ids,
            } => self.apply_linked_verifiable_presentations_removed(service_id, &presentation_ids),
        }
    }
}
#[cfg(test)]
pub mod service_tests {
    use super::*;
    // Imported by name rather than by glob: the fixture deliberately shares its name with the
    // production builder it wraps, and an explicit import says which one this module means.
    use super::test_utils::{
        linked_domains_resource, linked_domains_service, linked_domains_service_entry, linked_domains_service_id,
        linked_verifiable_presentation_service_id,
    };
    use agent_shared::config::set_config;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use cqrs_es::test::TestFramework;
    use identity_kit::{did::ServiceEndpoint, domain_linkage::UnverifiedLinkage, vc::Jwt};
    use rstest::{fixture, rstest};
    use serde_json::json;

    type ServiceTestFramework = TestFramework<Service>;

    /// The verification methods of the document fixtures, as the service commands take them.
    #[fixture]
    fn both_verification_methods() -> Vec<VerificationMethod> {
        crate::document::aggregate::test_utils::both_verification_methods::default()
    }

    fn web_did_method() {
        set_config().set_preferred_did_method(agent_shared::config::SupportedDidMethod::Web);
    }

    fn url(origin: &str) -> Url {
        origin.parse().unwrap()
    }

    /// A Domain Linkage Credential claiming `origin`, decodable but not signed.
    ///
    /// Unlinking a domain reads only the origin claim and never verifies a signature — that is what
    /// lets it keep the remaining credentials without re-issuing them — so a synthetic credential is
    /// exactly what these tests need.
    fn credential(origin: &Url) -> Jwt {
        let claims = DomainLinkageCredentialBuilder::new(
            "did:web:my-domain.example.org".parse().unwrap(),
            origin,
            test_utils::issuance_date(),
            test_utils::expiration_date(),
        )
        .unwrap()
        .jwt_claims();
        format!(
            "eyJhbGciOiJFZERTQSJ9.{}.c2lnbmF0dXJl",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
        .parse()
        .unwrap()
    }

    fn linked_dids(resource: &ServiceResource) -> Vec<Jwt> {
        let ServiceResource::LinkedDomains(configuration) = resource;
        configuration.jwts().map(|(_, jwt)| jwt.clone()).collect()
    }

    /// The origin each published credential claims, in the order they were issued.
    fn claimed_origins(resource: &ServiceResource) -> Vec<String> {
        linked_dids(resource)
            .iter()
            .map(|jwt| {
                UnverifiedLinkage::from_jwt(jwt)
                    .expect("credential claims an origin")
                    .origin
            })
            .collect()
    }

    /// An event linking `origins`, with one credential per origin, as a starting state.
    fn already_linked(service_id: &str, origins: &[&str]) -> ServiceEvent {
        let origins: Vec<Url> = origins.iter().map(|origin| url(origin)).collect();
        ServiceEvent::LinkedDomainsAdded {
            service_id: service_id.to_owned(),
            service: linked_domains_service_entry(service_id, &origins),
            resource: ServiceResource::LinkedDomains(origins.iter().map(credential).fold(
                DomainLinkageConfiguration::new(),
                DomainLinkageConfiguration::with_credential,
            )),
            is_deleted: false,
            origins,
        }
    }

    #[rstest]
    #[serial_test::serial]
    async fn linking_one_domain_writes_the_endpoint_as_a_bare_origin_string(
        linked_domains_service_id: String,
        both_verification_methods: Vec<VerificationMethod>,
        linked_domains_service: DocumentService,
        linked_domains_resource: ServiceResource,
    ) {
        web_did_method();

        ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ServiceCommand::AddLinkedDomains {
                service_id: linked_domains_service_id.clone(),
                verification_methods: both_verification_methods,
                origins: vec![url("https://my-domain.example.org")],
            })
            .then_expect_events(vec![ServiceEvent::LinkedDomainsAdded {
                service_id: linked_domains_service_id,
                // The fixture writes the flattened, single-origin form the specification allows.
                service: linked_domains_service,
                resource: linked_domains_resource,
                is_deleted: false,
                origins: vec![url("https://my-domain.example.org")],
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn linking_several_domains_issues_a_credential_per_domain_and_key(
        linked_domains_service_id: String,
        both_verification_methods: Vec<VerificationMethod>,
    ) {
        web_did_method();
        let method_count = both_verification_methods.len();
        assert!(method_count > 1, "this test needs more than one signing key");

        let events = ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            // Deliberately out of order, to show the stored set is normalized.
            .when(ServiceCommand::AddLinkedDomains {
                service_id: linked_domains_service_id.clone(),
                verification_methods: both_verification_methods,
                origins: vec![url("https://b.example"), url("https://a.example")],
            })
            .inspect_result()
            .expect("linking succeeds");

        let [ServiceEvent::LinkedDomainsAdded {
            service,
            resource,
            origins,
            ..
        }] = events.as_slice()
        else {
            panic!("expected a single LinkedDomainsAdded event, got {events:?}");
        };

        assert_eq!(origins, &[url("https://a.example"), url("https://b.example")]);
        // Each credential claims exactly one origin, so every (origin, key) pair gets its own.
        assert_eq!(
            claimed_origins(resource),
            vec![
                "https://a.example",
                "https://a.example",
                "https://b.example",
                "https://b.example"
            ]
        );
        assert_eq!(linked_dids(resource).len(), 2 * method_count);
        // Multiple origins use the array form rather than the flattened string.
        assert_eq!(
            serde_json::to_value(&service.service_endpoint).unwrap(),
            json!({ "origins": ["https://a.example", "https://b.example"] })
        );
    }

    #[rstest]
    #[serial_test::serial]
    async fn linking_a_domain_that_is_already_linked_changes_nothing(
        linked_domains_service_id: String,
        both_verification_methods: Vec<VerificationMethod>,
    ) {
        web_did_method();

        ServiceTestFramework::with(IdentityServices::default())
            .given(vec![already_linked(&linked_domains_service_id, &["https://a.example"])])
            .when(ServiceCommand::AddLinkedDomains {
                service_id: linked_domains_service_id,
                verification_methods: both_verification_methods,
                origins: vec![url("https://a.example")],
            })
            .then_expect_events(vec![])
    }

    #[rstest]
    #[serial_test::serial]
    async fn linking_a_further_domain_keeps_the_existing_ones(
        linked_domains_service_id: String,
        both_verification_methods: Vec<VerificationMethod>,
    ) {
        web_did_method();

        let events = ServiceTestFramework::with(IdentityServices::default())
            .given(vec![already_linked(&linked_domains_service_id, &["https://a.example"])])
            .when(ServiceCommand::AddLinkedDomains {
                service_id: linked_domains_service_id,
                verification_methods: both_verification_methods,
                origins: vec![url("https://b.example")],
            })
            .inspect_result()
            .expect("linking succeeds");

        let [ServiceEvent::LinkedDomainsAdded { origins, resource, .. }] = events.as_slice() else {
            panic!("expected a single LinkedDomainsAdded event, got {events:?}");
        };
        assert_eq!(origins, &[url("https://a.example"), url("https://b.example")]);
        assert_eq!(
            claimed_origins(resource)
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            ["https://a.example".to_string(), "https://b.example".to_string()].into()
        );
    }

    /// Unlinking keeps the remaining origins' credentials exactly as issued, which is also why it
    /// needs no signing keys at all.
    #[rstest]
    #[serial_test::serial]
    async fn unlinking_one_domain_keeps_the_others_credentials_verbatim(linked_domains_service_id: String) {
        web_did_method();
        let given = already_linked(&linked_domains_service_id, &["https://a.example", "https://b.example"]);
        let kept = credential(&url("https://a.example"));

        let events = ServiceTestFramework::with(IdentityServices::default())
            .given(vec![given])
            .when(ServiceCommand::RemoveLinkedDomains {
                service_id: linked_domains_service_id.clone(),
                origins: vec![url("https://b.example")],
            })
            .inspect_result()
            .expect("unlinking succeeds");

        let [ServiceEvent::LinkedDomainsRemoved {
            service,
            resource,
            is_deleted,
            origins,
            ..
        }] = events.as_slice()
        else {
            panic!("expected a single LinkedDomainsRemoved event, got {events:?}");
        };

        assert!(!is_deleted);
        assert_eq!(origins, &[url("https://a.example")]);
        assert_eq!(linked_dids(resource.as_ref().unwrap()), vec![kept]);
        // Back down to one origin, so the endpoint returns to the flattened form.
        assert_eq!(
            service.as_ref().unwrap().service_endpoint,
            ServiceEndpoint::Uri("https://a.example".parse().unwrap())
        );
    }

    #[rstest]
    #[serial_test::serial]
    async fn unlinking_the_last_domain_removes_the_service(linked_domains_service_id: String) {
        web_did_method();

        ServiceTestFramework::with(IdentityServices::default())
            .given(vec![already_linked(&linked_domains_service_id, &["https://a.example"])])
            .when(ServiceCommand::RemoveLinkedDomains {
                service_id: linked_domains_service_id.clone(),
                origins: vec![url("https://a.example")],
            })
            .then_expect_events(vec![ServiceEvent::LinkedDomainsRemoved {
                service_id: linked_domains_service_id,
                service: None,
                resource: None,
                is_deleted: true,
                origins: vec![],
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn unlinking_a_domain_that_is_not_linked_changes_nothing(linked_domains_service_id: String) {
        web_did_method();

        ServiceTestFramework::with(IdentityServices::default())
            .given(vec![already_linked(&linked_domains_service_id, &["https://a.example"])])
            .when(ServiceCommand::RemoveLinkedDomains {
                service_id: linked_domains_service_id,
                origins: vec![url("https://elsewhere.example")],
            })
            .then_expect_events(vec![])
    }

    #[rstest]
    #[serial_test::serial]
    async fn unlinking_when_nothing_is_linked_changes_nothing(linked_domains_service_id: String) {
        web_did_method();

        ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ServiceCommand::RemoveLinkedDomains {
                service_id: linked_domains_service_id,
                origins: vec![url("https://a.example")],
            })
            .then_expect_events(vec![])
    }

    #[rstest]
    #[serial_test::serial]
    async fn renewal_re_issues_the_linked_set_without_changing_it(
        linked_domains_service_id: String,
        both_verification_methods: Vec<VerificationMethod>,
    ) {
        web_did_method();

        let events = ServiceTestFramework::with(IdentityServices::default())
            .given(vec![already_linked(
                &linked_domains_service_id,
                &["https://a.example", "https://b.example"],
            )])
            .when(ServiceCommand::RenewLinkedDomainsCredentials {
                service_id: linked_domains_service_id,
                verification_methods: both_verification_methods,
                only_if_expiring: false,
            })
            .inspect_result()
            .expect("renewal succeeds");

        let [ServiceEvent::LinkedDomainsCredentialsRenewed { origins, resource, .. }] = events.as_slice() else {
            panic!("expected a single LinkedDomainsCredentialsRenewed event, got {events:?}");
        };
        assert_eq!(origins, &[url("https://a.example"), url("https://b.example")]);
        assert_eq!(
            claimed_origins(resource),
            vec![
                "https://a.example",
                "https://a.example",
                "https://b.example",
                "https://b.example"
            ]
        );
    }

    #[rstest]
    #[serial_test::serial]
    async fn linking_without_an_origin_is_rejected(
        linked_domains_service_id: String,
        both_verification_methods: Vec<VerificationMethod>,
    ) {
        web_did_method();

        ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ServiceCommand::AddLinkedDomains {
                service_id: linked_domains_service_id,
                verification_methods: both_verification_methods,
                origins: vec![],
            })
            .then_expect_error_message("At least one origin is required.")
    }

    #[rstest]
    #[serial_test::serial]
    async fn removing_presentations_keeps_the_remaining_order(linked_verifiable_presentation_service_id: String) {
        let presentations = ["a", "b", "c"].map(|id| LinkedVerifiablePresentation {
            presentation_id: id.into(),
            holder: "did:web:my-domain.example.org".into(),
            url: format!("https://my-domain.example.org/linked-verifiable-presentations/{id}")
                .parse()
                .unwrap(),
        });
        ServiceTestFramework::with(IdentityServices::default())
            .given(vec![ServiceEvent::LinkedVerifiablePresentationsAdded {
                service_id: linked_verifiable_presentation_service_id.clone(),
                presentations: presentations.to_vec(),
            }])
            .when(ServiceCommand::RemoveLinkedVerifiablePresentations {
                presentation_ids: vec!["b".into(), "missing".into()],
            })
            .then_expect_events(vec![ServiceEvent::LinkedVerifiablePresentationsRemoved {
                service_id: linked_verifiable_presentation_service_id,
                presentation_ids: vec!["b".into()],
            }])
    }

    #[test]
    fn applying_presentation_deltas_keeps_order_and_skips_published_ones() {
        let presentation = |id: &str| LinkedVerifiablePresentation {
            presentation_id: id.into(),
            holder: "did:web:my-domain.example.org".into(),
            url: format!("https://my-domain.example.org/linked-verifiable-presentations/{id}")
                .parse()
                .unwrap(),
        };
        let service_id = || LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.to_string();
        let mut service = Service::default();

        service.apply(ServiceEvent::LinkedVerifiablePresentationsAdded {
            service_id: service_id(),
            presentations: vec![presentation("a"), presentation("b"), presentation("c")],
        });
        service.apply(ServiceEvent::LinkedVerifiablePresentationsAdded {
            service_id: service_id(),
            presentations: vec![presentation("c"), presentation("d")],
        });
        assert_eq!(service.presentations, ["a", "b", "c", "d"].map(presentation).to_vec());

        service.apply(ServiceEvent::LinkedVerifiablePresentationsRemoved {
            service_id: service_id(),
            presentation_ids: vec!["b".into(), "d".into()],
        });
        assert_eq!(service.presentations, ["a", "c"].map(presentation).to_vec());
        assert!(!service.is_deleted);

        service.apply(ServiceEvent::LinkedVerifiablePresentationsRemoved {
            service_id: service_id(),
            presentation_ids: vec!["a".into(), "c".into()],
        });
        assert!(service.presentations.is_empty());
        assert!(service.is_deleted);

        service.apply(ServiceEvent::LinkedVerifiablePresentationsAdded {
            service_id: service_id(),
            presentations: vec![presentation("e")],
        });
        assert!(!service.is_deleted);
        assert!(service.is_active());
    }

    #[rstest]
    #[serial_test::serial]
    async fn removing_the_last_presentations_deletes_the_service(linked_verifiable_presentation_service_id: String) {
        let presentation = LinkedVerifiablePresentation {
            presentation_id: "a".into(),
            holder: "did:web:my-domain.example.org".into(),
            url: "https://my-domain.example.org/linked-verifiable-presentations/a"
                .parse()
                .unwrap(),
        };
        ServiceTestFramework::with(IdentityServices::default())
            .given(vec![ServiceEvent::LinkedVerifiablePresentationsAdded {
                service_id: linked_verifiable_presentation_service_id.clone(),
                presentations: vec![presentation],
            }])
            .when(ServiceCommand::RemoveLinkedVerifiablePresentations {
                presentation_ids: vec!["a".into(), "a".into()],
            })
            .then_expect_events(vec![ServiceEvent::LinkedVerifiablePresentationsRemoved {
                service_id: linked_verifiable_presentation_service_id,
                presentation_ids: vec!["a".into()],
            }])
    }

    #[test]
    fn presentation_url_treats_the_id_as_one_encoded_path_segment() {
        let url =
            linked_verifiable_presentation_url(&"https://example.org/unicore/".parse().unwrap(), "contains/?# a space")
                .unwrap();

        assert_eq!(
            url.as_str(),
            "https://example.org/unicore/linked-verifiable-presentations/contains%2F%3F%23%20a%20space"
        );
    }
}

#[cfg(test)]
mod replay_tests {
    use super::*;
    use cqrs_es::{EventEnvelope, View};

    #[test]
    fn persisted_service_events_rebuild_the_same_aggregate_and_projection() {
        let service_id = crate::state::LINKED_DOMAINS_SERVICE_ID;
        let origins = vec!["https://my-domain.example.org".parse::<Url>().unwrap()];
        let added = ServiceEvent::LinkedDomainsAdded {
            service_id: service_id.into(),
            service: test_utils::linked_domains_service(test_utils::linked_domains_service_id()),
            resource: test_utils::linked_domains_resource(),
            is_deleted: false,
            origins: origins.clone(),
        };
        let removed = ServiceEvent::LinkedDomainsRemoved {
            service_id: service_id.into(),
            service: None,
            resource: None,
            is_deleted: true,
            origins: vec![],
        };
        let mut aggregate = Service::default();
        let mut projection = Service::default();
        for (index, event) in [added.clone(), removed, added].into_iter().enumerate() {
            let persisted = serde_json::to_string(&event).unwrap();
            let restored: ServiceEvent = serde_json::from_str(&persisted).unwrap();
            aggregate.apply(restored.clone());
            projection.update(&EventEnvelope {
                aggregate_id: service_id.into(),
                sequence: index + 1,
                payload: restored,
                metadata: Default::default(),
            });
            assert_eq!(
                serde_json::to_value(&aggregate).unwrap(),
                serde_json::to_value(&projection).unwrap()
            );
            assert_eq!(aggregate.is_active(), index != 1);
            // Unlinking the last domain leaves nothing linked; relinking restores the set.
            assert_eq!(aggregate.origins.is_empty(), index == 1);
        }
    }

    #[test]
    fn linked_verifiable_presentation_removal_and_recreation_replay_consistently() {
        let presentations = vec![LinkedVerifiablePresentation {
            presentation_id: "presentation-1".into(),
            holder: "did:web:my-domain.example.org".into(),
            url: "https://my-domain.example.org/linked-verifiable-presentations/presentation-1"
                .parse()
                .unwrap(),
        }];
        let created = ServiceEvent::LinkedVerifiablePresentationsAdded {
            service_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
            presentations: presentations.clone(),
        };
        let removed = ServiceEvent::LinkedVerifiablePresentationsRemoved {
            service_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
            presentation_ids: vec!["presentation-1".into()],
        };
        let mut aggregate = Service::default();
        let mut projection = Service::default();
        for (index, event) in [created.clone(), removed, created].into_iter().enumerate() {
            aggregate.apply(event.clone());
            projection.update(&EventEnvelope {
                aggregate_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
                sequence: index + 1,
                payload: event,
                metadata: Default::default(),
            });
            assert_eq!(
                serde_json::to_value(&aggregate).unwrap(),
                serde_json::to_value(&projection).unwrap()
            );
            assert_eq!(aggregate.is_active(), index != 1);
        }
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use crate::state::{LINKED_DOMAINS_SERVICE_ID, LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID};
    use rstest::*;

    #[fixture]
    pub fn linked_domains_service_id() -> String {
        LINKED_DOMAINS_SERVICE_ID.to_string()
    }

    #[fixture]
    pub fn linked_verifiable_presentation_service_id() -> String {
        LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.to_string()
    }

    /// The `LinkedDomains` entry for `origins`, built exactly as the aggregate builds it.
    pub fn linked_domains_service_entry(service_id: &str, origins: &[Url]) -> DocumentService {
        super::linked_domains_service(service_id, origins).unwrap()
    }

    #[fixture]
    pub fn linked_domains_service(linked_domains_service_id: String) -> DocumentService {
        linked_domains_service_entry(
            &linked_domains_service_id,
            &["https://my-domain.example.org".parse().unwrap()],
        )
    }

    #[fixture]
    pub fn linked_domains_resource() -> ServiceResource {
        let configuration = DomainLinkageConfiguration::new().with_credential("eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDp3ZWI6bXktZG9tYWluLmV4YW1wbGUub3JnI2tleS0wIn0.eyJleHAiOjMxNTM2MDAwLCJpc3MiOiJkaWQ6d2ViOm15LWRvbWFpbi5leGFtcGxlLm9yZyIsIm5iZiI6MCwic3ViIjoiZGlkOndlYjpteS1kb21haW4uZXhhbXBsZS5vcmciLCJ2YyI6eyJAY29udGV4dCI6WyJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIsImh0dHBzOi8vaWRlbnRpdHkuZm91bmRhdGlvbi8ud2VsbC1rbm93bi9kaWQtY29uZmlndXJhdGlvbi92MSJdLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIiwiRG9tYWluTGlua2FnZUNyZWRlbnRpYWwiXSwiY3JlZGVudGlhbFN1YmplY3QiOnsib3JpZ2luIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvIn19fQ.l7dEPioa-No5zBlDCthfXDcffRB7371OnLrrQQgeAdnvHhs5F8XqRtdAWKXB8z3Se00WtGxHrTepLKmH9OWJDQ".parse().unwrap());

        ServiceResource::LinkedDomains(configuration)
    }

    pub fn issuance_date() -> DateTime<Utc> {
        DateTime::UNIX_EPOCH
    }

    pub fn expiration_date() -> DateTime<Utc> {
        issuance_date() + TimeDelta::days(365)
    }
}
