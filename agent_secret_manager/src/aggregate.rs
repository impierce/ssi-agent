use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};

use crate::commands::SecretManagerCommand;
use crate::events::SecretManagerEvent;
use crate::services::SecretManagerServices;

#[derive(Serialize, Deserialize)]
pub struct AgentSecretManager {
    // TODO: problem: SecretManager is not serializable --> use service?
    // secret_manager: SecretManager,
}

#[async_trait]
impl Aggregate for AgentSecretManager {
    type Command = SecretManagerCommand;
    type Event = SecretManagerEvent;
    type Error = std::io::Error;
    type Services = Arc<Mutex<SecretManagerServices>>;

    fn aggregate_type() -> String {
        "secret_manager".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            SecretManagerCommand::LoadStronghold => {
                let mut guard = services.lock().await;
                assert!(guard.secret_manager.is_none());
                guard.init().unwrap();
                assert!(guard.secret_manager.is_some());

                Ok(vec![SecretManagerEvent::StrongholdLoaded {}])
            }
            SecretManagerCommand::EnableDidMethod { method } => {
                let guard = services.lock().await;
                assert!(guard.secret_manager.is_some());
                let result = guard
                    .secret_manager
                    .as_ref()
                    .unwrap()
                    .produce_document_json(method.clone())
                    .await;

                if result.is_ok() {
                    Ok(vec![SecretManagerEvent::DidMethodEnabled { method }])
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Failed to enable DID method",
                    ))
                }
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {}
}

impl Default for AgentSecretManager {
    fn default() -> Self {
        AgentSecretManager {}
    }
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    use cqrs_es::test::TestFramework;
    use producer::did_document::Method;
    use producer::SecretManager;

    use crate::aggregate::AgentSecretManager;
    use crate::commands::SecretManagerCommand;
    use crate::events::SecretManagerEvent;
    use crate::services::SecretManagerServices;

    type SecretManagerTestFramework = TestFramework<AgentSecretManager>;

    #[test]
    fn loads_stronghold() {
        std::env::set_var("AGENT_SECRET_MANAGER_STRONGHOLD_PATH", "tests/res/test.stronghold");
        std::env::set_var("AGENT_SECRET_MANAGER_STRONGHOLD_PASSWORD", "secure_password");

        let expected = SecretManagerEvent::StrongholdLoaded {};
        let command = SecretManagerCommand::LoadStronghold;
        let services = Arc::new(Mutex::new(SecretManagerServices::new(None)));

        SecretManagerTestFramework::with(services)
            .given_no_previous_events()
            .when(command)
            .then_expect_events(vec![expected])
    }

    #[test]
    fn enables_did_method() {
        let expected = SecretManagerEvent::DidMethodEnabled { method: Method::Key };
        let command = SecretManagerCommand::EnableDidMethod { method: Method::Key };
        let services = Arc::new(Mutex::new(SecretManagerServices::new(Some(
            SecretManager::load("tests/res/test.stronghold".to_string(), "secure_password".to_string()).unwrap(),
        ))));

        SecretManagerTestFramework::with(services)
            .given_no_previous_events()
            .when(command)
            .then_expect_events(vec![expected])
    }
}
