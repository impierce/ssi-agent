use super::command::AuthorizationCodeCommand;
use super::error::AuthorizationCodeError;
use super::event::AuthorizationCodeEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct AuthorizationCode {
    #[serde(rename = "id")]
    pub authorization_code_id: String,
}

#[async_trait]
impl Aggregate for AuthorizationCode {
    type Command = AuthorizationCodeCommand;
    type Event = AuthorizationCodeEvent;
    type Error = AuthorizationCodeError;
    type Services = ();

    fn aggregate_type() -> String {
        "authorization_code".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use AuthorizationCodeCommand::*;
        use AuthorizationCodeError::*;
        use AuthorizationCodeEvent::*;

        info!("Handling command: {:?}", command);

        match command {}
    }

    fn apply(&mut self, event: Self::Event) {
        use AuthorizationCodeEvent::*;

        debug!("Applying event: {:?}", event);

        match event {}
    }
}

#[cfg(test)]
pub mod authorization_code_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
