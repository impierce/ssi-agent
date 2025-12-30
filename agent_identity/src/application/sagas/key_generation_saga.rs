use agent_shared::handlers::{command_handler, query_handler};
use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};
use identity_iota::verification::VerificationMethod;
use identity_storage::KeyId;

use crate::{
    document::command::DocumentCommand,
    managed_key::{
        aggregate::{ManagedKey, SigningAlgorithm},
        command::ManagedKeyCommand,
    },
    services::IdentityServices,
    state::IdentityState,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct KeyGenerationSaga {
    identity_state: Arc<IdentityState>,
    identity_services: Arc<IdentityServices>,
}

impl KeyGenerationSaga {
    pub fn new(identity_state: Arc<IdentityState>, identity_services: Arc<IdentityServices>) -> Self {
        Self {
            identity_state,
            identity_services,
        }
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

        command_handler(&managed_key_id, &self.identity_state.command.managed_key, command).await?;

        if let Some(managed_key_view) = query_handler(&managed_key_id, &self.identity_state.query.managed_key).await? {
            let key_id = managed_key_view.key_id.clone();
            let signing_algorithm = managed_key_view.signing_algorithm.unwrap();

            if let Some(all_documents_view) =
                query_handler("all_documents", &self.identity_state.query.all_documents).await?
            {
                for (document_id, document_view) in all_documents_view.documents {
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
