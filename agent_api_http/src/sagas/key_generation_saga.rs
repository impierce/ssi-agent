use agent_secret_manager::{
    managed_key::{aggregate::SigningAlgorithm, command::ManagedKeyCommand},
    state::SecretManagerState,
};
use agent_shared::handlers::{command_handler, query_handler};
use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};
use identity_iota::verification::VerificationMethod;
use identity_storage::KeyId;

use agent_identity::{
    document::command::DocumentCommand,
    service::command::ServiceCommand,
    services::IdentityServices,
    state::{IdentityState, DOMAIN_LINKAGE_SERVICE_ID},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct KeyGenerationSaga {
    secret_manager_state: Arc<SecretManagerState>,
    identity_state: Arc<IdentityState>,
    identity_services: Arc<IdentityServices>,
}

impl KeyGenerationSaga {
    pub fn new(
        secret_manager_state: Arc<SecretManagerState>,
        identity_state: Arc<IdentityState>,
        identity_services: Arc<IdentityServices>,
    ) -> Self {
        Self {
            secret_manager_state,
            identity_state,
            identity_services,
        }
    }

    pub async fn generate_default_keys(&self) -> Result<(), Box<dyn std::error::Error>> {
        let current_keys_n = query_handler("all_managed_keys", &self.secret_manager_state.query.all_managed_keys)
            .await?
            .map(|all_managed_keys_view| all_managed_keys_view.managed_keys.len())
            .unwrap_or_default();

        if current_keys_n == 0 {
            self.generate_key("EdDSA Key".to_string(), SigningAlgorithm::EdDSA)
                .await?;
            self.generate_key("ES256 Key".to_string(), SigningAlgorithm::ES256)
                .await?;
        }

        Ok(())
    }

    pub async fn generate_key(
        &self,
        alias: String,
        signing_algorithm: SigningAlgorithm,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // TODO: Add undo logic!!!

        let managed_key_id = uuid::Uuid::new_v4().to_string();

        let command = ManagedKeyCommand::GenerateKey {
            managed_key_id: managed_key_id.clone(),
            alias,
            signing_algorithm,
        };

        command_handler(&managed_key_id, &self.secret_manager_state.command.managed_key, command).await?;

        if let Some(managed_key_view) =
            query_handler(&managed_key_id, &self.secret_manager_state.query.managed_key).await?
        {
            let key_id = managed_key_view.key_id.clone();
            let signing_algorithm = managed_key_view.signing_algorithm.unwrap();

            if let Some(all_documents_view) =
                query_handler("all_documents", &self.identity_state.query.all_documents).await?
            {
                for (document_id, document_view) in &all_documents_view.documents {
                    if document_view
                        .did_method
                        .map(|did_method| did_method.supports_update())
                        .unwrap_or(false)
                    {
                        let command = DocumentCommand::AddVerificationMethod {
                            key_id: key_id.clone(),
                            signing_algorithm: signing_algorithm.clone(),
                        };

                        command_handler(&document_id, &self.identity_state.command.document, command).await?;
                    }
                }

                let command = ServiceCommand::CreateDomainLinkageService {
                    service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                    verification_methods: vec![],
                };

                command_handler(
                    &DOMAIN_LINKAGE_SERVICE_ID,
                    &self.identity_state.command.service,
                    command,
                )
                .await
                .ok();

                let domain_linkage_service =
                    query_handler(DOMAIN_LINKAGE_SERVICE_ID, &self.identity_state.query.service).await?;

                for (document_id, document_view) in all_documents_view.documents {
                    if document_view
                        .did_method
                        .map(|did_method| did_method.supports_update())
                        .unwrap_or(false)
                    {
                        if let Some(domain_linkage_service) = domain_linkage_service
                            .as_ref()
                            .and_then(|domain_linkage_service| domain_linkage_service.service.clone())
                        {
                            let command = DocumentCommand::AddService {
                                service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                                service: Box::new(domain_linkage_service),
                            };

                            command_handler(&document_id, &self.identity_state.command.document, command).await?;
                        }

                        if document_view
                            .did_method
                            .map(|did_method| did_method.hosted_decentrally())
                            .unwrap_or(false)
                        {
                            let command = DocumentCommand::PublishDocument;

                            command_handler(&document_id, &self.identity_state.command.document, command).await?;
                        }
                    }
                }
            }
        }
        Ok(managed_key_id)
    }
}
