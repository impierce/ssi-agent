use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};

use crate::commands::SecretManagerCommand;
use crate::events::SecretManagerEvent;
use crate::services::SecretManagerServices;

/// An aggregate that uses services to interact with `did_manager::SecretManager`.
#[derive(Serialize, Deserialize, Default)]
pub struct AgentSecretManager {}

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
            SecretManagerCommand::Initialize => {
                let mut guard = services.lock().await;
                assert!(guard.subject.is_none());
                guard.init().await.unwrap();
                assert!(guard.subject.is_some());

                Ok(vec![SecretManagerEvent::Initialized {}])
            }
            SecretManagerCommand::EnableDidMethod { method } => {
                let guard = services.lock().await;
                assert!(guard.subject.is_some());
                let result = guard
                    .subject
                    .as_ref()
                    .unwrap()
                    .secret_manager
                    .produce_document(method.clone())
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

    fn apply(&mut self, _event: Self::Event) {}
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    use cqrs_es::test::TestFramework;
    use did_manager::DidMethod;

    use crate::aggregate::AgentSecretManager;
    use crate::commands::SecretManagerCommand;
    use crate::events::SecretManagerEvent;
    use crate::services::SecretManagerServices;

    type SecretManagerTestFramework = TestFramework<AgentSecretManager>;

    #[test]
    fn successfully_initializes_secret_manager() {
        let expected = SecretManagerEvent::Initialized {};
        let command = SecretManagerCommand::Initialize;
        let services = Arc::new(Mutex::new(SecretManagerServices::new(None)));

        SecretManagerTestFramework::with(services)
            .given_no_previous_events()
            .when(command)
            .then_expect_events(vec![expected])
    }

    #[test]
    fn successfully_enables_did_method() {
        let expected = SecretManagerEvent::DidMethodEnabled { method: DidMethod::Key };
        let command = SecretManagerCommand::EnableDidMethod { method: DidMethod::Key };
        let services = futures::executor::block_on(async {
            let mut services = SecretManagerServices::new(None);
            services.init().await.unwrap();
            Arc::new(Mutex::new(services))
        });

        SecretManagerTestFramework::with(services)
            .given_no_previous_events()
            .when(command)
            .then_expect_events(vec![expected])
    }
}
