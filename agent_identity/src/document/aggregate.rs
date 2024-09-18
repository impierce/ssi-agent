use std::sync::Arc;

use agent_shared::{
    config::{config, get_preferred_signing_algorithm},
    from_jsonwebtoken_algorithm_to_jwsalgorithm,
};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use did_manager::{DidMethod, MethodSpecificParameters};
use identity_document::document::CoreDocument;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::services::IdentityServices;

use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Document {
    pub document: Option<CoreDocument>,
}

#[async_trait]
impl Aggregate for Document {
    type Command = DocumentCommand;
    type Event = DocumentEvent;
    type Error = DocumentError;
    type Services = Arc<IdentityServices>;

    fn aggregate_type() -> String {
        "credential".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use DocumentCommand::*;
        use DocumentError::*;
        use DocumentEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateDocument { did_method } => {
                let mut secret_manager = services.subject.secret_manager.lock().await;

                let method_specific_parameters =
                    matches!(did_method, DidMethod::Web).then(|| MethodSpecificParameters::Web {
                        origin: config().url.origin(),
                    });

                let document = secret_manager
                    .produce_document(
                        did_method,
                        method_specific_parameters,
                        // TODO: This way the Document can only support on single algorithm. We need to support multiple algorithms.
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    // FIX THISS
                    .unwrap();

                Ok(vec![DocumentCreated { document }])
            }
            AddService { service } => {
                // FIX THIS
                let mut document = self.document.clone().unwrap();

                // FIX THIS
                document.insert_service(service).unwrap();

                Ok(vec![ServiceAdded { document }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use DocumentEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            DocumentCreated { document } => {
                self.document.replace(document);
            }
            ServiceAdded { document } => {
                self.document.replace(document);
            }
        }
    }
}
