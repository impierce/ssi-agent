use crate::key::{command::KeyCommand, error::KeyError, event::KeyEvent};
use crate::key_provider::aggregate::KeyProviderType;
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_kit::jose::{Jwk, JwsAlgorithm};
use serde::{Deserialize, Serialize};
use tracing::info;
use utoipa::ToSchema;

/// Where a key lives and what it is. A reference never changes once the key is created: new key material always
/// gets a new reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KeyReference {
    pub provider_type: KeyProviderType,
    /// The provider's identifier for the key.
    pub provider_key_id: String,
    pub algorithm: JwsAlgorithm,
    pub public_key: Jwk,
}

/// A key this instance created in one of its key providers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Key {
    pub reference: Option<KeyReference>,
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
        info!("Handling command: {:?}", command);

        let event = match command {
            KeyCommand::RecordCreatedKey { reference } => {
                if self.reference.is_some() {
                    return Err(KeyError::AlreadyRecorded);
                }
                KeyEvent::KeyCreated { reference }
            }
        };
        sink.write(event, self).await;

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            KeyEvent::KeyCreated { reference } => self.reference = Some(reference),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::test::TestFramework;

    fn reference(provider_key_id: &str) -> KeyReference {
        KeyReference {
            provider_type: KeyProviderType::InMemory,
            provider_key_id: provider_key_id.to_string(),
            algorithm: JwsAlgorithm::EdDSA,
            // The public key of RFC 8032's first Ed25519 test vector.
            public_key: Jwk::from_ed25519_public(&[
                0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a, 0x0e,
                0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
            ])
            .unwrap(),
        }
    }

    #[test]
    fn a_key_is_recorded_only_once() {
        TestFramework::<Key>::with(())
            .given(vec![KeyEvent::KeyCreated {
                reference: reference("first"),
            }])
            .when(KeyCommand::RecordCreatedKey {
                reference: reference("second"),
            })
            .then_expect_error(KeyError::AlreadyRecorded);
    }
}
