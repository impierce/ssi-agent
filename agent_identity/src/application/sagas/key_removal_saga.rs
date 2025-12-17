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

pub struct KeyRemovalSaga {
    identity_state: Arc<IdentityState>,
    identity_services: Arc<IdentityServices>,
}

impl KeyRemovalSaga {
    pub fn new(identity_state: Arc<IdentityState>, identity_services: Arc<IdentityServices>) -> Self {
        Self {
            identity_state,
            identity_services,
        }
    }

    pub async fn remove_key(&self, managed_key_id: String) {
        // TODO: Add undo logic!!!

        let command = ManagedKeyCommand::RemoveKey;

        command_handler(&managed_key_id, &self.identity_state.command.managed_key, command)
            .await
            .unwrap();

        if let Some(managed_key_view) = query_handler(&managed_key_id, &self.identity_state.query.managed_key)
            .await
            .unwrap()
        {
            let key_id = managed_key_view.key_id.clone();

            if let Some(all_documents_view) = query_handler("all_documents", &self.identity_state.query.all_documents)
                .await
                .unwrap()
            {
                for (document_id, document_view) in all_documents_view.documents {
                    if document_view
                        .did_method
                        .map(|did_method| did_method.supports_update())
                        .unwrap_or(false)
                    {
                        let command = DocumentCommand::RemoveVerificationMethod { key_id: key_id.clone() };

                        command_handler(&document_id, &self.identity_state.command.document, command)
                            .await
                            .unwrap();

                        if document_view
                            .did_method
                            .map(|did_method| did_method.hosted_decentrally())
                            .unwrap_or(false)
                        {
                            let command = DocumentCommand::PublishDocument;

                            command_handler(&document_id, &self.identity_state.command.document, command)
                                .await
                                .unwrap();
                        }
                    }
                }
            }
        }
    }
}
