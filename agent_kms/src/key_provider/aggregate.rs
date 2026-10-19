use crate::key_provider::{
    command::KeyProviderCommand, error::KeyProviderError, event::KeyProviderEvent, services::KeyProviderServices,
};
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strum::{Display, EnumString};
use tracing::info;
use utoipa::ToSchema;

/// The kinds of key providers an instance can connect to. An instance connects to at most one provider of each type,
/// so the type also identifies the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, ToSchema)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum KeyProviderType {
    /// Keeps keys in memory. Only for development and tests: keys are lost on restart.
    InMemory,
    /// AWS KMS, reached with the AWS credentials and region of the environment `UniCore` runs in.
    AwsKmsAmbient,
}

impl KeyProviderType {
    /// Whether this provider may only be used when `UniCore` runs with the `development` profile.
    #[must_use]
    pub fn is_development_only(self) -> bool {
        matches!(self, Self::InMemory)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct KeyProvider {
    pub provider_type: Option<KeyProviderType>,
    pub name: String,
}

impl KeyProvider {
    fn is_added(&self) -> bool {
        self.provider_type.is_some()
    }
}

impl Aggregate for KeyProvider {
    type Command = KeyProviderCommand;
    type Event = KeyProviderEvent;
    type Error = KeyProviderError;
    type Services = Arc<KeyProviderServices>;

    const TYPE: &'static str = "key_provider";

    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use KeyProviderCommand::{AddKeyProvider, RemoveKeyProvider};
        use KeyProviderEvent::{KeyProviderAdded, KeyProviderRemoved};

        info!("Handling command: {:?}", command);

        let events = match command {
            AddKeyProvider { provider_type, name } => {
                if self.is_added() {
                    return Err(KeyProviderError::AlreadyAdded(provider_type));
                }
                if provider_type.is_development_only() && !services.development_providers_allowed {
                    return Err(KeyProviderError::DevelopmentOnly(provider_type));
                }

                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err(KeyProviderError::MissingName);
                }
                if services.is_name_taken(&name).await? {
                    return Err(KeyProviderError::NameTaken(name));
                }

                vec![KeyProviderAdded { provider_type, name }]
            }
            RemoveKeyProvider { provider_type } => {
                if !self.is_added() {
                    return Err(KeyProviderError::NotFound(provider_type));
                }

                vec![KeyProviderRemoved { provider_type }]
            }
        };

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            KeyProviderEvent::KeyProviderAdded { provider_type, name } => {
                self.provider_type = Some(provider_type);
                self.name = name;
            }
            KeyProviderEvent::KeyProviderRemoved { .. } => *self = Self::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::test::TestFramework;

    type KeyProviderTestFramework = TestFramework<KeyProvider>;

    fn services(development_providers_allowed: bool) -> Arc<KeyProviderServices> {
        Arc::new(KeyProviderServices::new(development_providers_allowed))
    }

    fn added(provider_type: KeyProviderType, name: &str) -> KeyProviderEvent {
        KeyProviderEvent::KeyProviderAdded {
            provider_type,
            name: name.to_string(),
        }
    }

    #[test]
    fn adds_a_provider_with_a_trimmed_name() {
        KeyProviderTestFramework::with(services(false))
            .given_no_previous_events()
            .when(KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::AwsKmsAmbient,
                name: "  Production keys ".to_string(),
            })
            .then_expect_events(vec![added(KeyProviderType::AwsKmsAmbient, "Production keys")]);
    }

    #[test]
    fn rejects_a_second_provider_of_the_same_type() {
        KeyProviderTestFramework::with(services(false))
            .given(vec![added(KeyProviderType::AwsKmsAmbient, "Production keys")])
            .when(KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::AwsKmsAmbient,
                name: "Other keys".to_string(),
            })
            .then_expect_error(KeyProviderError::AlreadyAdded(KeyProviderType::AwsKmsAmbient));
    }

    #[test]
    fn rejects_an_empty_name() {
        KeyProviderTestFramework::with(services(false))
            .given_no_previous_events()
            .when(KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::AwsKmsAmbient,
                name: "   ".to_string(),
            })
            .then_expect_error(KeyProviderError::MissingName);
    }

    #[test]
    fn rejects_the_in_memory_provider_outside_development() {
        KeyProviderTestFramework::with(services(false))
            .given_no_previous_events()
            .when(KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::InMemory,
                name: "Local keys".to_string(),
            })
            .then_expect_error(KeyProviderError::DevelopmentOnly(KeyProviderType::InMemory));
    }

    #[test]
    fn allows_the_in_memory_provider_in_development() {
        KeyProviderTestFramework::with(services(true))
            .given_no_previous_events()
            .when(KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::InMemory,
                name: "Local keys".to_string(),
            })
            .then_expect_events(vec![added(KeyProviderType::InMemory, "Local keys")]);
    }

    #[test]
    fn removes_an_added_provider() {
        KeyProviderTestFramework::with(services(false))
            .given(vec![added(KeyProviderType::AwsKmsAmbient, "Production keys")])
            .when(KeyProviderCommand::RemoveKeyProvider {
                provider_type: KeyProviderType::AwsKmsAmbient,
            })
            .then_expect_events(vec![KeyProviderEvent::KeyProviderRemoved {
                provider_type: KeyProviderType::AwsKmsAmbient,
            }]);
    }

    #[test]
    fn can_add_a_provider_again_after_removing_it() {
        KeyProviderTestFramework::with(services(false))
            .given(vec![
                added(KeyProviderType::AwsKmsAmbient, "Production keys"),
                KeyProviderEvent::KeyProviderRemoved {
                    provider_type: KeyProviderType::AwsKmsAmbient,
                },
            ])
            .when(KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::AwsKmsAmbient,
                name: "Production keys".to_string(),
            })
            .then_expect_events(vec![added(KeyProviderType::AwsKmsAmbient, "Production keys")]);
    }

    #[test]
    fn rejects_removing_a_provider_that_was_never_added() {
        KeyProviderTestFramework::with(services(false))
            .given_no_previous_events()
            .when(KeyProviderCommand::RemoveKeyProvider {
                provider_type: KeyProviderType::AwsKmsAmbient,
            })
            .then_expect_error(KeyProviderError::NotFound(KeyProviderType::AwsKmsAmbient));
    }
}
