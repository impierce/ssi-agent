use agent_shared::config::{config, SupportedDidMethod, ToggleOptions};
use agent_shared::handlers::command_handler;
use agent_shared::{application_state::CommandHandler, handlers::query_handler};
use cqrs_es::persist::ViewRepository;
use did_manager::DidMethod;
use futures::future::{join_all, try_join_all};
use jsonwebtoken::Algorithm;
use oid4vc_core::Subject;
use std::sync::Arc;
use tracing::{info, warn};

use crate::connection::aggregate::Connection;
use crate::connection::views::all_connections::AllConnectionsView;
use crate::connection::views::ConnectionView;
use crate::document::command::DocumentCommand;
use crate::service::views::all_services::AllServicesView;
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
    pub connection: CommandHandler<Connection>,
    pub document: CommandHandler<Document>,
    pub service: CommandHandler<Service>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn ViewRepository<ConnectionView, Connection>,
    dyn ViewRepository<AllConnectionsView, Connection>,
    dyn ViewRepository<DocumentView, Document>,
    dyn ViewRepository<ServiceView, Service>,
    dyn ViewRepository<AllServicesView, Service>,
>;

pub struct ViewRepositories<C1, C2, D, S1, S2>
where
    C1: ViewRepository<ConnectionView, Connection> + ?Sized,
    C2: ViewRepository<AllConnectionsView, Connection> + ?Sized,
    D: ViewRepository<DocumentView, Document> + ?Sized,
    S1: ViewRepository<ServiceView, Service> + ?Sized,
    S2: ViewRepository<AllServicesView, Service> + ?Sized,
{
    pub connection: Arc<C1>,
    pub all_connections: Arc<C2>,
    pub document: Arc<D>,
    pub service: Arc<S1>,
    pub all_services: Arc<S2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            connection: self.connection.clone(),
            all_connections: self.all_connections.clone(),
            document: self.document.clone(),
            service: self.service.clone(),
            all_services: self.all_services.clone(),
        }
    }
}

/// The unique identifier for the linked domain service.
pub const DOMAIN_LINKAGE_SERVICE_ID: &str = "linked-domain-service";

/// The unique identifier for the linked verifiable presentation service.
pub const VERIFIABLE_PRESENTATION_SERVICE_ID: &str = "linked-verifiable-presentation-service";

/// Initialize the identity state.
pub async fn initialize(state: &IdentityState, subject: Arc<dyn Subject>) {
    info!("Initializing ...");

    // Only consider non-deterministic DID methods that are enabled.
    let did_methods = config()
        .did_methods
        .clone()
        .into_iter()
        .filter(|(did_method, toggle_options)| !did_method.is_deterministic() && toggle_options.enabled)
        .collect::<Vec<_>>();
    let documents = try_join_all(
        // Loop through all DID methods.
        did_methods
            .iter()
            .map(|(did_method, _)| async  {
                // Clone the variables into the async closure.
                let did_method = did_method.clone();
                    let document_id = did_method.to_string();

                    // Check whether the DID methods document already exists.
                    match query_handler(&did_method.to_string(), &state.query.document).await {
                        Ok(Some(Document {
                            document: Some(document),
                            ..
                        })) => {
                            // TODO: FIX THISS
                            let key_id = subject.key_id(&did_method.to_string(), Algorithm::ES256).await.unwrap();
                            let condition = document.verification_method().iter().any(|vm| {
                                info!("vm.id().to_string() == key_id: {} == {}", vm.id().to_string(), key_id);
                                vm.id().to_string() == key_id});

                            if condition {
                                return Err(format!("2: DID Document for `{}` already exists, but the identifier does not match the subject identifier", did_method));
                            } else {
                                info!("3: DID Document for `{did_method}` already exists: {:?}", document);
                            }
                        }
                        // If the DID document does not exist yet, then it needs to be created.
                        _document_does_not_exist => {
                            info!("4: Creating new DID Document for `{did_method}`");


                            let command = DocumentCommand::CreateDocument {
                                document_id: document_id.clone(),
                            };

                            if command_handler(&document_id, &state.command.document, command)
                                .await
                                .is_err()
                            {
                                warn!("5: Failed to create DID Document for `{did_method}`");
                            }

                            info!("6: Created document for `{}`", did_method);
                        }
                    }

                    match query_handler(&did_method.to_string(), &state.query.document).await {
                        Ok(Some(document)) => Ok(document),
                        _ => Err(format!("DID Document for `{}` does not exist", did_method)),
                    }


            })
            .collect::<Vec<_>>(),
    )
    .await
    .unwrap();

    if config().domain_linkage_enabled {
        let command = ServiceCommand::CreateDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
            documents,
        };

        if command_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command)
            .await
            .is_err()
        {
            warn!("Failed to create domain linkage service");
        }

        match query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await {
            Ok(domain_linkage_service) => {
                try_join_all(
                    // Loop through all DID methods.
                    did_methods
                        .iter()
                        .map(|(did_method, _)| async {
                            // Clone the variables into the async closure.
                            let did_method = did_method.clone();
                            let document_id = did_method.to_string();
                            let domain_linkage_service = domain_linkage_service.clone();

                            if let Some(Service {
                                type_: Some(type_),
                                service_endpoint: Some(service_endpoint),
                                ..
                            }) = domain_linkage_service
                            {
                                let command = DocumentCommand::AddService {
                                    service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                                    type_,
                                    service_endpoint,
                                };

                                if command_handler(&did_method.to_string(), &state.command.document, command)
                                    .await
                                    .is_err()
                                {
                                    warn!("7: Failed to add service to document");
                                }

                                info!("8: Added service to document for `{}`", did_method);
                            }

                            if did_method.is_external() {
                                let command = DocumentCommand::PublishDocument {
                                    document_id: document_id.clone(),
                                };

                                if command_handler(&document_id, &state.command.document, command)
                                    .await
                                    .is_err()
                                {
                                    warn!("9: Failed to publish DID Document for `{did_method}`");
                                }
                            }

                            info!("10: Published document for `{}`", did_method);

                            Ok::<(), ()>(())
                        })
                        .collect::<Vec<_>>(),
                )
                .await
                .unwrap();
            }
            _ => {
                warn!("Failed to retrieve linked domains service");
                return;
            }
        };
    }
}
