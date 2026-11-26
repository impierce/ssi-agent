use crate::key::{command::KeyCommand, error::KeyError, event::KeyEvent};
use crate::key_provider::{aggregate::KeyProviderType, settings::KeyProviderSettings};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_kit::{
    jose::{Jwk, JwsAlgorithm},
    keys::KeyId,
};
use serde::{Deserialize, Serialize};

/// What this instance knows about a key in one of its providers. The private key never leaves the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyReference {
    pub provider_type: KeyProviderType,
    /// The settings the provider had when the key was recorded, which say where in the provider the key is. The key
    /// is reached with them, also once the provider is removed or added again with other settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<KeyProviderSettings>,
    /// The provider's id for the key, which names this key for good: an alias can later name another key, so for
    /// AWS KMS this is the key ARN.
    pub provider_key_id: KeyId,
    /// The label that named the key in the provider when it was recorded, if any.
    pub label: Option<String>,
    /// The one algorithm the key signs with.
    pub algorithm: JwsAlgorithm,
    pub public_key: Jwk,
}

/// A key in one of this instance's key providers, identified by an id of this instance's own. A key is recorded once,
/// when it is created or adopted, and stays deleted once deleted. What the key is lives in its events and views.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Key {
    #[default]
    Unrecorded,
    Recorded,
    Deleted,
}

impl Aggregate for Key {
    type Command = KeyCommand;
    type Event = KeyEvent;
    type Error = KeyError;
    type Services = ();

    const TYPE: &'static str = "key";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        let event = match (command, &*self) {
            (KeyCommand::CreateKey { reference }, Key::Unrecorded) => KeyEvent::KeyCreated { reference },
            (KeyCommand::AdoptKey { reference }, Key::Unrecorded) => KeyEvent::KeyAdopted { reference },
            (KeyCommand::CreateKey { .. } | KeyCommand::AdoptKey { .. }, _) => return Err(KeyError::AlreadyRecorded),
            (KeyCommand::DeleteKey, Key::Recorded) => KeyEvent::KeyDeleted,
            (KeyCommand::DeleteKey, _) => return Err(KeyError::NotFound),
        };

        sink.write(event, self).await;
        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        *self = match event {
            KeyEvent::KeyCreated { .. } | KeyEvent::KeyAdopted { .. } => Key::Recorded,
            KeyEvent::KeyDeleted => Key::Deleted,
        };
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cqrs_es::test::TestFramework;

    /// An ES256 key in the in-memory provider.
    pub(crate) fn reference() -> KeyReference {
        KeyReference {
            provider_type: KeyProviderType::InMemory,
            settings: None,
            provider_key_id: KeyId::new("issuer"),
            label: Some("issuer".to_string()),
            algorithm: JwsAlgorithm::ES256,
            public_key: serde_json::from_value(serde_json::json!({
                "kty": "EC",
                "crv": "P-256",
                "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
                "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"
            }))
            .unwrap(),
        }
    }

    #[test]
    fn a_key_is_recorded_only_once() {
        TestFramework::<Key>::with(())
            .given(vec![KeyEvent::KeyCreated { reference: reference() }])
            .when(KeyCommand::AdoptKey { reference: reference() })
            .then_expect_error(KeyError::AlreadyRecorded);
    }

    #[test]
    fn a_deleted_key_cannot_be_deleted_again() {
        TestFramework::<Key>::with(())
            .given(vec![
                KeyEvent::KeyCreated { reference: reference() },
                KeyEvent::KeyDeleted,
            ])
            .when(KeyCommand::DeleteKey)
            .then_expect_error(KeyError::NotFound);
    }
}
