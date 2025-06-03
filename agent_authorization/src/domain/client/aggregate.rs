use super::command::ClientCommand;
use super::error::ClientError;
use super::event::ClientEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Client {
    #[serde(rename = "id")]
    pub client_id: String,
}

#[async_trait]
impl Aggregate for Client {
    type Command = ClientCommand;
    type Event = ClientEvent;
    type Error = ClientError;
    type Services = ();

    fn aggregate_type() -> String {
        "client".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ClientCommand::*;
        use ClientError::*;
        use ClientEvent::*;

        info!("Handling command: {:?}", command);

        match command {}
    }

    fn apply(&mut self, event: Self::Event) {
        use ClientEvent::*;

        debug!("Applying event: {:?}", event);

        match event {}
    }
}

#[cfg(test)]
pub mod client_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
