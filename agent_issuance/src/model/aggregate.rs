use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};

use crate::{
    command::IssuanceCommand, error::IssuanceError, event::IssuanceEvent, service::IssuanceServices,
};

#[derive(Serialize, Default, Deserialize)]
struct CredentialTemplate {
    schema: serde_json::Value,
}

#[derive(Serialize, Default, Deserialize)]
pub struct Credential {
    credential_template: CredentialTemplate,
    credential_data: serde_json::Value,
    // TODO: add proof?
    // proof: Option<T>
}

#[async_trait]
impl Aggregate for Credential {
    type Command = IssuanceCommand;
    type Event = IssuanceEvent;
    type Error = IssuanceError;
    type Services = IssuanceServices;

    fn aggregate_type() -> String {
        "Credential".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        todo!()
    }

    fn apply(&mut self, event: Self::Event) {
        use IssuanceEvent::*;
        match event {
            CredentialDataCreated { .. } => todo!(),
            CredentialSigned { .. } => todo!(),
        }
    }
}
