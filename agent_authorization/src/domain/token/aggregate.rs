use super::command::TokenCommand;
use super::error::TokenError;
use super::event::TokenEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Token {
    #[serde(rename = "id")]
    pub token_id: String,
}

#[async_trait]
impl Aggregate for Token {
    type Command = TokenCommand;
    type Event = TokenEvent;
    type Error = TokenError;
    type Services = ();

    fn aggregate_type() -> String {
        "token".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use TokenCommand::*;
        use TokenError::*;
        use TokenEvent::*;

        info!("Handling command: {:?}", command);

        match command {}
    }

    fn apply(&mut self, event: Self::Event) {
        use TokenEvent::*;

        debug!("Applying event: {:?}", event);

        match event {}
    }
}

#[cfg(test)]
pub mod token_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
