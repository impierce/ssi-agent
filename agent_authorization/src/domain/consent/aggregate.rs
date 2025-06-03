use super::command::ConsentCommand;
use super::error::ConsentError;
use super::event::ConsentEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Consent {
    #[serde(rename = "id")]
    pub consent_id: String,
}

#[async_trait]
impl Aggregate for Consent {
    type Command = ConsentCommand;
    type Event = ConsentEvent;
    type Error = ConsentError;
    type Services = ();

    fn aggregate_type() -> String {
        "consent".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ConsentCommand::*;
        use ConsentError::*;
        use ConsentEvent::*;

        info!("Handling command: {:?}", command);

        match command {}
    }

    fn apply(&mut self, event: Self::Event) {
        use ConsentEvent::*;

        debug!("Applying event: {:?}", event);

        match event {}
    }
}

#[cfg(test)]
pub mod consent_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
