use agent_shared::config::{config, SupportedDidMethod, ToggleOptions};
use agent_shared::handlers::command_handler;
use agent_shared::{application_state::CommandHandler, handlers::query_handler};
use cqrs_es::persist::ViewRepository;
use did_manager::DidMethod;
use std::sync::Arc;
use tracing::{info, warn};

use crate::document::command::DocumentCommand;
use crate::{
    document::{aggregate::Document, views::DocumentView},
    service::{aggregate::Service, command::ServiceCommand, views::ServiceView},
};

#[derive(Clone)]
pub struct IdentityState {
    pub command: CommandHandlers,
    pub query: Queries,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub document: CommandHandler<Document>,
    pub service: CommandHandler<Service>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<dyn ViewRepository<DocumentView, Document>, dyn ViewRepository<ServiceView, Service>>;

pub struct ViewRepositories<D, S>
where
    D: ViewRepository<DocumentView, Document> + ?Sized,
    S: ViewRepository<ServiceView, Service> + ?Sized,
{
    pub document: Arc<D>,
    pub service: Arc<S>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            document: self.document.clone(),
            service: self.service.clone(),
        }
    }
}

/// The unique identifier for the linked domain service.
pub const DOMAIN_LINKAGE_SERVICE_ID: &str = "linked-domain-service";

/// Initialize the identity state.
pub async fn initialize(state: &IdentityState) {
    info!("Initializing ...");

    let enable_did_web = config()
        .did_methods
        .get(&SupportedDidMethod::Web)
        .unwrap_or(&ToggleOptions::default())
        .enabled;

    // If the did:web method is enabled, create a document
    if enable_did_web {
        let did_method = DidMethod::Web;
        let command = DocumentCommand::CreateDocument {
            did_method: did_method.clone(),
        };

        if command_handler(&did_method.to_string(), &state.command.document, command)
            .await
            .is_err()
        {
            warn!("Failed to create document");
        }

        // If domain linkage is enabled, create the domain linkage service and add it to the document.
        // TODO: Support this for other (non-deterministic) DID methods.
        if config().domain_linkage_enabled {
            let command = ServiceCommand::CreateDomainLinkageService {
                service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
            };

            if command_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command)
                .await
                .is_err()
            {
                warn!("Failed to create domain linkage service");
            }

            let linked_domains_service = match query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await {
                Ok(Some(Service {
                    service: Some(linked_domains_service),
                    ..
                })) => linked_domains_service,
                _ => {
                    warn!("Failed to retrieve linked domains service");
                    return;
                }
            };

            let command = DocumentCommand::AddService {
                service: linked_domains_service,
            };

            if command_handler(&did_method.to_string(), &state.command.document, command)
                .await
                .is_err()
            {
                warn!("Failed to add service to document");
            }
        }
    }
}
