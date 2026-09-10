use super::{
    aggregate::{Service, ServiceResource},
    command::ServiceCommand,
    error::ServiceError,
};
use crate::dns::CnameCheck;
use crate::{
    document::{
        aggregate::{Document, Status},
        command::DocumentCommand,
    },
    services::linked_dids_by_origin,
    state::{publish_decentrally_hosted_documents, query_all_documents, IdentityState, LINKED_DOMAINS_SERVICE_ID},
};
use agent_shared::handlers::{public_command_handler, public_query_handler, CommandHandlerError};
use identity_did::DID as _;
use serde::Serialize;
use shared_kernel::authorization::{
    Actor, AuthorizationError, AuthorizationOperation, AuthorizationRequest, CommandAuthorization,
};
use std::sync::{Arc, Weak};
use tracing::warn;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum ServiceManagementError {
    #[error(transparent)]
    Authorization(#[from] AuthorizationError),
    #[error(transparent)]
    Command(#[from] CommandHandlerError<ServiceError>),
    #[error(transparent)]
    Infrastructure(#[from] anyhow::Error),
}

/// Authorizes the complete service operation, including its derived document updates.
pub async fn execute(
    state: &IdentityState,
    actor: Option<Actor>,
    command: ServiceCommand,
) -> Result<(), ServiceManagementError> {
    state
        .authorization_checker
        .is_authorized(&AuthorizationRequest {
            actor,
            operation: AuthorizationOperation::Command {
                aggregate_id: command.service_id().to_owned(),
                command_type: command.operation(),
                authorization: CommandAuthorization::ACTOR_REQUIRED,
            },
        })
        .await?;
    let _guard = state.service_lifecycle_lock.lock().await;
    execute_locked(state, command).await
}

fn can_link(document: &Document) -> bool {
    document.status != Status::Disabled
        && document.did_method.is_some_and(|method| method.supports_update())
        && document.iota_metadata.as_ref().is_none_or(|metadata| {
            metadata.is_funded || agent_shared::config::config().iota_sponsoring_service_url.is_some()
        })
}

async fn execute_locked(state: &IdentityState, mut command: ServiceCommand) -> Result<(), ServiceManagementError> {
    let documents = query_all_documents(state, |_| true).await?;
    match &mut command {
        // Unlinking deliberately needs no verification methods: it keeps the remaining origins'
        // existing credentials rather than re-signing, so it still works once signing keys are gone.
        ServiceCommand::AddLinkedDomains {
            verification_methods, ..
        }
        | ServiceCommand::RenewLinkedDomainsCredentials {
            verification_methods, ..
        } => {
            *verification_methods = documents
                .values()
                .filter(|document| can_link(document))
                .filter_map(|document| document.document.as_ref())
                .flat_map(|document| document.methods(None).into_iter().cloned())
                .collect();
        }
        _ => {}
    }
    let service_id = command.service_id().to_owned();
    // The actor was checked at the use-case boundary; these writes implement that operation.
    public_command_handler(&service_id, &state.command.service, command).await?;
    synchronize_services(state).await?;
    Ok(())
}

/// Reconciles document entries with persisted service state, including interrupted removals.
async fn synchronize_services(state: &IdentityState) -> anyhow::Result<()> {
    let services = public_query_handler("all_services", &state.query.all_services)
        .await?
        .unwrap_or_default();
    let documents = query_all_documents(state, |_| true).await?;
    for service in services.services.values() {
        for document in documents.values() {
            let Some(core_document) = &document.document else {
                continue;
            };
            if !document.did_method.is_some_and(|method| method.supports_update()) {
                continue;
            }
            let command = if !service.is_active() {
                if core_document.resolve_service(service.service_id.as_str()).is_none() {
                    continue;
                }
                DocumentCommand::RemoveService {
                    service_id: service.service_id.clone(),
                }
            } else {
                if !can_link(document) {
                    continue;
                }
                let mut entry = service.service.clone().expect("active service has an entry");
                entry.set_id(core_document.id().to_url().join(format!("#{}", service.service_id))?)?;
                if core_document.resolve_service(service.service_id.as_str()) == Some(&entry) {
                    continue;
                }
                DocumentCommand::AddService {
                    service_id: service.service_id.clone(),
                    service: Box::new(entry),
                }
            };
            public_command_handler(&document.document_id, &state.command.document, command).await?;
        }
    }
    publish_decentrally_hosted_documents(state).await
}

async fn linked_domains(state: &IdentityState) -> anyhow::Result<Option<Service>> {
    Ok(public_query_handler(LINKED_DOMAINS_SERVICE_ID, &state.query.service).await?)
}

/// The outcome of verifying every linked domain, mirroring what an external verifier would see.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct LinkedDomainsVerification {
    /// Whether every linked domain verified.
    pub valid: bool,
    /// Why verification could not be attempted at all, e.g. because no domain is linked. `None` when
    /// the per-origin results below carry the outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub origins: Vec<LinkedDomainVerification>,
}

impl LinkedDomainsVerification {
    fn failure(message: impl Into<String>) -> Self {
        Self {
            valid: false,
            message: Some(message.into()),
            origins: Vec::new(),
        }
    }
}

/// The outcome of verifying one linked domain.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct LinkedDomainVerification {
    #[schema(value_type = String, example = "https://example.org")]
    pub origin: Url,
    /// Whether an external verifier resolving this origin would accept the linkage. This is the
    /// authoritative result; `dns` is a diagnostic, because a domain can be served correctly without
    /// a `CNAME` record — an apex domain cannot have one.
    pub valid: bool,
    /// Whether this origin's published configuration validates against the DIDs UniCore linked to it.
    pub linkage_valid: bool,
    /// Whether this origin's DNS points at the deployment.
    pub dns: CnameCheck,
    /// What went wrong, or `None` when this origin verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Authorizes, then verifies every linked domain the way an external verifier would: for each origin
/// it resolves that origin's `/.well-known/did-configuration.json` over the network and validates it
/// against the DIDs UniCore linked to *that* origin, proving DNS, HTTPS and the `/.well-known/`
/// hosting are genuinely reachable rather than merely internally consistent.
///
/// Each origin's `CNAME` record is resolved fresh alongside it. That check cannot be authoritative —
/// apex domains have no `CNAME` — so it only explains a failure rather than causing one.
pub async fn verify(
    state: &IdentityState,
    actor: Option<Actor>,
) -> Result<LinkedDomainsVerification, ServiceManagementError> {
    state
        .authorization_checker
        .is_authorized(&AuthorizationRequest {
            actor,
            operation: AuthorizationOperation::Query {
                query_type: std::any::type_name::<LinkedDomainsVerification>(),
            },
        })
        .await?;

    let Some(Service {
        is_deleted: false,
        resource: Some(ServiceResource::LinkedDomains(config)),
        origins,
        ..
    }) = linked_domains(state).await?
    else {
        return Ok(LinkedDomainsVerification::failure("No domains are linked."));
    };

    // Which DIDs UniCore published for which origin. Each credential claims exactly one origin, so
    // coverage has to be checked per (origin, DID) pair rather than per DID.
    let expected = linked_dids_by_origin(&config);
    if expected.is_empty() {
        return Ok(LinkedDomainsVerification::failure("No linked DIDs are configured."));
    }

    let deployment = state.services.public_url.clone();
    let mut results = Vec::with_capacity(origins.len());

    for origin in origins {
        let dns = CnameCheck::resolve(&state.services.cname_resolver, &origin, &deployment).await;
        let expected_dids = expected.get(&origin).cloned().unwrap_or_default();

        let (linkage_valid, mut problems) = if expected_dids.is_empty() {
            (
                false,
                vec!["no Domain Linkage Credential was issued for this origin".to_string()],
            )
        } else {
            match state.services.fetch_linked_dids(&origin).await {
                Ok(actual) => {
                    let problems: Vec<String> = expected_dids
                        .iter()
                        .filter_map(|did| match actual.iter().find(|linked| linked.did.did() == did.did()) {
                            Some(linked) if linked.domain_linkage_valid => None,
                            Some(linked) => Some(format!(
                                "{did}: {}",
                                linked
                                    .domain_linkage_error
                                    .clone()
                                    .unwrap_or_else(|| "domain linkage is invalid".to_string())
                            )),
                            None => Some(format!(
                                "{did}: not found in the published domain linkage configuration"
                            )),
                        })
                        .collect();
                    (problems.is_empty(), problems)
                }
                Err(error) => (
                    false,
                    vec![format!(
                        "Failed to fetch the published domain linkage configuration: {error}"
                    )],
                ),
            }
        };

        // A missing or misdirected CNAME is usually *why* the fetch failed, so it is reported
        // alongside the failure to make it actionable.
        if !linkage_valid {
            problems.extend(dns.failure(&deployment));
        }

        results.push(LinkedDomainVerification {
            origin,
            valid: linkage_valid,
            linkage_valid,
            dns,
            message: (!problems.is_empty()).then(|| problems.join("; ")),
        });
    }

    Ok(LinkedDomainsVerification {
        valid: results.iter().all(|result| result.valid),
        message: None,
        origins: results,
    })
}

pub async fn renew_existing_linked_domains_credentials(state: &IdentityState) -> anyhow::Result<()> {
    let _guard = state.service_lifecycle_lock.lock().await;
    if linked_domains(state).await?.is_some_and(|service| service.is_active()) {
        execute_locked(state, renewal_command(false)).await?;
    }
    Ok(())
}

fn renewal_command(only_if_expiring: bool) -> ServiceCommand {
    ServiceCommand::RenewLinkedDomainsCredentials {
        service_id: LINKED_DOMAINS_SERVICE_ID.into(),
        verification_methods: vec![],
        only_if_expiring,
    }
}

/// Startup and runtime share the same renewal policy. Deleted services are never renewed.
pub async fn maintain_services(state: &IdentityState) -> anyhow::Result<()> {
    let _guard = state.service_lifecycle_lock.lock().await;
    if linked_domains(state)
        .await?
        .is_some_and(|service| service.needs_renewal((state.services.linkage_clock)()))
    {
        let eligible_documents = query_all_documents(state, |(_, document)| can_link(document)).await?;
        if eligible_documents.is_empty() {
            warn!("Linked domains need renewal, but no eligible signing DID is enabled");
        } else {
            execute_locked(state, renewal_command(true)).await?;
            return Ok(());
        }
    }
    synchronize_services(state).await?;
    Ok(())
}

/// A weak reference lets maintenance stop when the application state is dropped.
pub fn spawn_maintenance(state: &Arc<IdentityState>) -> tokio::task::JoinHandle<()> {
    let state: Weak<IdentityState> = Arc::downgrade(state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(state) = state.upgrade() else { break };
            if let Err(error) = maintain_services(&state).await {
                warn!("Identity service maintenance failed; retrying in one hour: {error:#}");
            }
        }
    })
}
