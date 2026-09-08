use super::{
    aggregate::{Service, ServiceResource},
    command::ServiceCommand,
    error::ServiceError,
};
use crate::{
    document::{
        aggregate::{Document, Status},
        command::DocumentCommand,
    },
    services::extract_linked_dids,
    state::{publish_decentrally_hosted_documents, query_all_documents, IdentityState, DOMAIN_LINKAGE_SERVICE_ID},
};
use agent_shared::handlers::{public_command_handler, public_query_handler, CommandHandlerError};
use identity_did::DID as _;
use serde::Serialize;
use shared_kernel::authorization::{
    Actor, AuthorizationError, AuthorizationOperation, AuthorizationRequest, CommandAuthorization,
};
use std::sync::{Arc, Weak};
use tracing::warn;

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
        ServiceCommand::CreateDomainLinkageService {
            verification_methods, ..
        }
        | ServiceCommand::ReissueDomainLinkageService {
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

async fn domain_linkage(state: &IdentityState) -> anyhow::Result<Option<Service>> {
    Ok(public_query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await?)
}

/// The outcome of resolving UniCore's currently published domain linkage over the network and
/// validating it, mirroring what an external verifier would see.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct DomainLinkageVerification {
    pub valid: bool,
    pub message: Option<String>,
}

impl DomainLinkageVerification {
    fn failure(message: impl Into<String>) -> Self {
        Self {
            valid: false,
            message: Some(message.into()),
        }
    }
}

/// Authorizes, then resolves the domain linkage configuration UniCore currently publishes at its
/// own `public_url` and validates it against the DID(s) it expects to have linked, i.e. the ones
/// recorded in the persisted `DomainLinkageConfiguration`. This proves that DNS, HTTPS, and the
/// `/.well-known/` hosting are actually reachable and correctly configured from the outside,
/// rather than merely checking internal consistency.
pub async fn verify(
    state: &IdentityState,
    actor: Option<Actor>,
) -> Result<DomainLinkageVerification, ServiceManagementError> {
    state
        .authorization_checker
        .is_authorized(&AuthorizationRequest {
            actor,
            operation: AuthorizationOperation::Command {
                aggregate_id: DOMAIN_LINKAGE_SERVICE_ID.to_owned(),
                command_type: "identity.services.domain_linkage.verify",
                authorization: CommandAuthorization::ACTOR_REQUIRED,
            },
        })
        .await?;

    let Some(Service {
        is_deleted: false,
        resource: Some(ServiceResource::DomainLinkage(config)),
        ..
    }) = domain_linkage(state).await?
    else {
        return Ok(DomainLinkageVerification::failure(
            "Domain linkage has not been created.",
        ));
    };

    let expected = extract_linked_dids(&config);
    if expected.is_empty() {
        return Ok(DomainLinkageVerification::failure("No linked DIDs are configured."));
    }

    let actual = match state.services.fetch_linked_dids(&state.services.public_url).await {
        Ok(actual) => actual,
        Err(error) => {
            return Ok(DomainLinkageVerification::failure(format!(
                "Failed to fetch the published domain linkage configuration: {error}"
            )));
        }
    };

    let problems: Vec<String> = expected
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

    if problems.is_empty() {
        Ok(DomainLinkageVerification {
            valid: true,
            message: None,
        })
    } else {
        Ok(DomainLinkageVerification::failure(problems.join("; ")))
    }
}

pub async fn reissue_existing_domain_linkage(state: &IdentityState) -> anyhow::Result<()> {
    let _guard = state.service_lifecycle_lock.lock().await;
    if domain_linkage(state).await?.is_some_and(|service| service.is_active()) {
        execute_locked(state, reissue_command(false)).await?;
    }
    Ok(())
}

fn reissue_command(only_if_expiring: bool) -> ServiceCommand {
    ServiceCommand::ReissueDomainLinkageService {
        service_id: DOMAIN_LINKAGE_SERVICE_ID.into(),
        verification_methods: vec![],
        only_if_expiring,
    }
}

/// Startup and runtime share the same renewal policy. Deleted services are never renewed.
pub async fn maintain_services(state: &IdentityState) -> anyhow::Result<()> {
    let _guard = state.service_lifecycle_lock.lock().await;
    if domain_linkage(state)
        .await?
        .is_some_and(|service| service.needs_renewal((state.services.linkage_clock)()))
    {
        let eligible_documents = query_all_documents(state, |(_, document)| can_link(document)).await?;
        if eligible_documents.is_empty() {
            warn!("Domain linkage needs renewal, but no eligible signing DID is enabled");
        } else {
            execute_locked(state, reissue_command(true)).await?;
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
