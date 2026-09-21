use super::{command::ServiceCommand, error::ServiceError, event::ServiceEvent};
use crate::services::IdentityServices;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_core::{
    common::{Duration, OrderedSet, Timestamp},
    convert::{FromJson, ToJson},
};
use identity_credential::{
    credential::Jwt,
    domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder},
};
use identity_did::DIDUrl;
use identity_document::service::{Service as DocumentService, ServiceEndpoint};
use identity_iota::verification::VerificationMethod;
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::Sign as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
    pub presentation_ids: Vec<String>,
    pub resource: Option<ServiceResource>,
    pub is_deleted: bool,
    /// The origins the published Domain Linkage Credentials claim, sorted and deduplicated. These
    /// are whatever the caller linked at runtime and bear no relation to the deployment's own DID.
    pub origins: Vec<Url>,
}

pub const LINKED_DOMAINS_VALIDITY_DAYS: u32 = 365;
pub const LINKED_DOMAINS_RENEWAL_WINDOW_DAYS: i64 = 30;

impl Service {
    pub fn is_active(&self) -> bool {
        self.service.is_some() && !self.is_deleted
    }

    pub fn needs_renewal(&self, now: Timestamp) -> bool {
        if !self.is_active() {
            return false;
        }
        let Some(ServiceResource::LinkedDomains(configuration)) = &self.resource else {
            return false;
        };
        let threshold = now.to_unix() + LINKED_DOMAINS_RENEWAL_WINDOW_DAYS * 86400;
        configuration.linked_dids().is_empty()
            || configuration.linked_dids().iter().any(|jwt| {
                let expiration = credential_claims(jwt)
                    .as_ref()
                    .and_then(|claims| claims.get("exp").and_then(serde_json::Value::as_i64));
                expiration.is_none_or(|expiration| expiration <= threshold)
            })
    }
}

/// Decodes a Domain Linkage Credential JWT's claims **without verifying its signature**.
///
/// Only ever used to read back claims UniCore itself signed and persisted, so the signature adds
/// nothing here; the credentials are served to verifiers intact.
fn credential_claims(jwt: &Jwt) -> Option<serde_json::Value> {
    jwt.as_str()
        .split('.')
        .nth(1)
        .and_then(|payload| URL_SAFE_NO_PAD.decode(payload).ok())
        .and_then(|payload| serde_json::from_slice(&payload).ok())
}

/// The origin a Domain Linkage Credential claims, used to tell one origin's credentials from
/// another's when unlinking a domain.
pub(crate) fn credential_origin(jwt: &Jwt) -> Option<Url> {
    credential_claims(jwt)?
        .get("vc")?
        .get("credentialSubject")?
        .get("origin")?
        .as_str()?
        .parse()
        .ok()
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

/// Renders an origin the way a Domain Linkage Credential must claim it.
fn credential_origin_url(origin: &Url) -> Result<identity_core::common::Url, ServiceError> {
    identity_core::common::Url::parse(canonical_origin(origin)?.as_str())
        .map_err(|err| ServiceError::InvalidUrlError(err.to_string()))
}

/// Builds the `LinkedDomains` service entry for `origins`.
///
/// The DID Configuration specification allows a `serviceEndpoint` to be *either* a bare origin string
/// or an object with an `origins` array, so a single linked domain is written in the simpler string
/// form and only multiple domains use the array.
fn linked_domains_service(service_id: &str, origins: &[Url]) -> Result<DocumentService, ServiceError> {
    use ServiceError::*;

    let origins = origins
        .iter()
        .map(credential_origin_url)
        .collect::<Result<Vec<_>, _>>()?;

    let service_endpoint = match origins.as_slice() {
        [] => return Err(EmptyOriginsError),
        [single] => ServiceEndpoint::One(single.clone()),
        many => ServiceEndpoint::from_json_value(json!({ "origins": many }))
            .map_err(|err| InvalidServiceEndpointError(err.to_string()))?,
    };

    DocumentService::builder(Default::default())
        // This service is DID method-agnostic. When added to an enabled DID Document, its
        // placeholder value is replaced with the appropriate DID method-specific identifier.
        .id(format!("did:place:holder#{service_id}")
            .parse::<DIDUrl>()
            .map_err(|err| InvalidDidError(err.to_string()))?)
        .type_("LinkedDomains")
        .service_endpoint(service_endpoint)
        .build()
        .map_err(|err| ServiceBuilderError(err.to_string()))
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
    issuance_date: Timestamp,
) -> Result<(DocumentService, ServiceResource), ServiceError> {
    use ServiceError::*;

    let subject = &services.subject;
    let expiration_date = issuance_date
        // TODO: make this configurable
        .checked_add(Duration::days(LINKED_DOMAINS_VALIDITY_DAYS))
        .ok_or(InvalidTimestampError)?;

    let mut linked_dids = vec![];

    for origin in origins {
        let origin = credential_origin_url(origin)?;

        for verification_method in &verification_methods {
            let subject_did = verification_method.id().did();

            let verification_method_id = verification_method.id();
            let alg = verification_method
                .data()
                .public_key_jwk()
                .and_then(|jwk| jwk.alg())
                .ok_or_else(|| MissingVerificationMethodAlgorithm(verification_method_id.to_string()))?;
            let algorithm =
                Algorithm::from_str(alg).map_err(|_| UnsupportedVerificationMethodAlgorithm(alg.to_string()))?;

            let domain_linkage_credential = DomainLinkageCredentialBuilder::new()
                .issuer(subject_did.clone())
                .origin(origin.clone())
                .issuance_date(issuance_date)
                .expiration_date(expiration_date)
                .build()
                .map_err(|err| DomainLinkageCredentialBuilderError(err.to_string()))?
                .serialize_jwt(Default::default())
                .map_err(|err| SerializationError(err.to_string()))?;

            // Compose JWT
            let header = Header {
                alg: algorithm,
                typ: None,
                kid: Some(verification_method.id().to_string()),
                ..Default::default()
            };

            let linked_did = [
                URL_SAFE_NO_PAD.encode(
                    header
                        .to_json_vec()
                        .map_err(|err| SerializationError(err.to_string()))?,
                ),
                URL_SAFE_NO_PAD.encode(domain_linkage_credential.as_bytes()),
            ]
            .join(".");

            let proof_value = subject
                // TODO: Currently UniCore always uses the same keys for signing regardless of the DID method.
                // Once we implement DID method-specific keys, then we should supply the appropriate
                // `subject_syntax_type` here instead of this `placeholder` value.
                .sign(linked_did.as_str(), "placeholder", algorithm)
                .await
                .map_err(|err| SigningError(err.to_string()))?;
            let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
            let linked_did = [linked_did, signature].join(".");

            linked_dids.push(Jwt::from(linked_did))
        }
    }

    if linked_dids.is_empty() {
        return Err(EmptyLinkedDidsError);
    }

    let configuration = DomainLinkageConfiguration::new(linked_dids);
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
                    let kept: Vec<Jwt> = configuration
                        .linked_dids()
                        .iter()
                        .filter(|jwt| credential_origin(jwt).is_some_and(|origin| !removed.contains(&origin)))
                        .cloned()
                        .collect();
                    if kept.is_empty() {
                        return Err(EmptyLinkedDidsError);
                    }
                    (
                        Some(linked_domains_service(&service_id, &remaining)?),
                        Some(ServiceResource::LinkedDomains(DomainLinkageConfiguration::new(kept))),
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
            DeleteLinkedVerifiablePresentationService { service_id } => {
                if !self.is_active() {
                    return Err(NotFound);
                }
                Ok(vec![LinkedVerifiablePresentationServiceDeleted { service_id }])
            }
            CreateLinkedVerifiablePresentationService {
                service_id,
                presentation_ids,
            } => {
                if self.is_active() {
                    return Err(AlreadyExists);
                }
                let origin = &services.public_url;

                let service_endpoint = ServiceEndpoint::from(OrderedSet::from_iter(
                    presentation_ids
                        .clone()
                        .into_iter()
                        .map(|presentation_id| {
                            // TODO: Find a better way to construct the URL
                            format!("{origin}linked-verifiable-presentations/{presentation_id}")
                                .parse::<identity_core::common::Url>()
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|err| InvalidUrlError(err.to_string()))?,
                ));

                // Create a new service.
                let service = DocumentService::builder(Default::default())
                    // This service is DID method-agnostic. When added to an enabled DID Document,
                    // its placeholder value is replaced with the appropriate DID method-specific identifier.
                    .id(format!("did:place:holder#{service_id}")
                        .parse::<DIDUrl>()
                        .map_err(|err| InvalidDidError(err.to_string()))?)
                    .type_("LinkedVerifiablePresentation")
                    .service_endpoint(service_endpoint)
                    .build()
                    .map_err(|err| ServiceBuilderError(err.to_string()))?;

                Ok(vec![LinkedVerifiablePresentationServiceCreated {
                    service_id,
                    presentation_ids,
                    service,
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
            LinkedVerifiablePresentationServiceDeleted { service_id } => {
                self.service_id = service_id;
                self.service = None;
                self.resource = None;
                self.presentation_ids.clear();
                self.is_deleted = true;
            }
            LinkedVerifiablePresentationServiceCreated {
                service_id,
                service,
                presentation_ids,
            } => {
                self.service_id = service_id;
                self.presentation_ids = presentation_ids;
                self.is_deleted = false;
                self.service.replace(service);
            }
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
        linked_verifiable_presentation_service, linked_verifiable_presentation_service_id,
    };
    use crate::document::aggregate::test_utils::both_verification_methods;
    use agent_shared::config::set_config;
    use cqrs_es::test::TestFramework;
    use identity_iota::verification::VerificationMethod;
    use rstest::rstest;

    type ServiceTestFramework = TestFramework<Service>;

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
        let payload = json!({
            "iss": "did:web:my-domain.example.org",
            "sub": "did:web:my-domain.example.org",
            "vc": { "credentialSubject": { "origin": origin.as_str() } },
        });
        Jwt::from(format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
        ))
    }

    fn linked_dids(resource: &ServiceResource) -> Vec<Jwt> {
        let ServiceResource::LinkedDomains(configuration) = resource;
        configuration.linked_dids().to_vec()
    }

    /// The origin each published credential claims, in the order they were issued.
    fn claimed_origins(resource: &ServiceResource) -> Vec<String> {
        linked_dids(resource)
            .iter()
            .map(|jwt| credential_origin(jwt).expect("credential claims an origin").to_string())
            .collect()
    }

    /// An event linking `origins`, with one credential per origin, as a starting state.
    fn already_linked(service_id: &str, origins: &[&str]) -> ServiceEvent {
        let origins: Vec<Url> = origins.iter().map(|origin| url(origin)).collect();
        ServiceEvent::LinkedDomainsAdded {
            service_id: service_id.to_owned(),
            service: linked_domains_service_entry(service_id, &origins),
            resource: ServiceResource::LinkedDomains(DomainLinkageConfiguration::new(
                origins.iter().map(credential).collect::<Vec<_>>(),
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
                "https://a.example/",
                "https://a.example/",
                "https://b.example/",
                "https://b.example/"
            ]
        );
        assert_eq!(linked_dids(resource).len(), 2 * method_count);
        // Multiple origins use the array form rather than the flattened string.
        assert_eq!(
            service.service_endpoint(),
            &ServiceEndpoint::from_json_value(json!({
                "origins": ["https://a.example/", "https://b.example/"]
            }))
            .unwrap()
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
            ["https://a.example/".to_string(), "https://b.example/".to_string()].into()
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
            service.as_ref().unwrap().service_endpoint(),
            &ServiceEndpoint::One("https://a.example/".parse().unwrap())
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
                "https://a.example/",
                "https://a.example/",
                "https://b.example/",
                "https://b.example/"
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
    async fn test_create_linked_verifiable_presentation_service(
        linked_verifiable_presentation_service_id: String,
        linked_verifiable_presentation_service: DocumentService,
    ) {
        ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ServiceCommand::CreateLinkedVerifiablePresentationService {
                service_id: linked_verifiable_presentation_service_id.clone(),
                presentation_ids: vec!["presentation-1".to_string()],
            })
            .then_expect_events(vec![ServiceEvent::LinkedVerifiablePresentationServiceCreated {
                service_id: linked_verifiable_presentation_service_id,
                presentation_ids: vec!["presentation-1".to_string()],
                service: linked_verifiable_presentation_service,
            }])
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
    fn linked_presentation_removal_and_recreation_replay_consistently() {
        let created = ServiceEvent::LinkedVerifiablePresentationServiceCreated {
            service_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
            presentation_ids: vec!["presentation-1".into()],
            service: test_utils::linked_verifiable_presentation_service(
                test_utils::linked_verifiable_presentation_service_id(),
            ),
        };
        let removed = ServiceEvent::LinkedVerifiablePresentationServiceDeleted {
            service_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
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
    use identity_document::service::Service;
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
    pub fn linked_verifiable_presentation_service(
        linked_verifiable_presentation_service_id: String,
    ) -> DocumentService {
        let origin = "https://my-domain.example.org";

        Service::builder(Default::default())
            .id(format!("did:place:holder#{linked_verifiable_presentation_service_id}")
                .parse()
                .unwrap())
            .type_("LinkedVerifiablePresentation")
            .service_endpoint(ServiceEndpoint::from(OrderedSet::from_iter(vec![format!(
                "{origin}/linked-verifiable-presentations/presentation-1"
            )
            .parse::<identity_core::common::Url>()
            .unwrap()])))
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn linked_domains_resource() -> ServiceResource {
        let configuration = DomainLinkageConfiguration::new(vec![Jwt::from("eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDp3ZWI6bXktZG9tYWluLmV4YW1wbGUub3JnI2tleS0wIn0.eyJleHAiOjMxNTM2MDAwLCJpc3MiOiJkaWQ6d2ViOm15LWRvbWFpbi5leGFtcGxlLm9yZyIsIm5iZiI6MCwic3ViIjoiZGlkOndlYjpteS1kb21haW4uZXhhbXBsZS5vcmciLCJ2YyI6eyJAY29udGV4dCI6WyJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIsImh0dHBzOi8vaWRlbnRpdHkuZm91bmRhdGlvbi8ud2VsbC1rbm93bi9kaWQtY29uZmlndXJhdGlvbi92MSJdLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIiwiRG9tYWluTGlua2FnZUNyZWRlbnRpYWwiXSwiY3JlZGVudGlhbFN1YmplY3QiOnsib3JpZ2luIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvIn19fQ.l7dEPioa-No5zBlDCthfXDcffRB7371OnLrrQQgeAdnvHhs5F8XqRtdAWKXB8z3Se00WtGxHrTepLKmH9OWJDQ".to_string())]);

        ServiceResource::LinkedDomains(configuration)
    }

    pub fn issuance_date() -> Timestamp {
        Timestamp::from_unix(0).unwrap()
    }

    pub fn expiration_date() -> Timestamp {
        issuance_date().checked_add(Duration::days(365)).unwrap()
    }
}
