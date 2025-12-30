use crate::services::IdentityServices;

use super::{command::ManagedKeyCommand, error::ManagedKeyError, event::ManagedKeyEvent};
use agent_secret_manager::subject::Subject;
use agent_shared::config::config;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::Aggregate;
use identity_core::{
    common::{Duration, OrderedSet, Timestamp},
    convert::{FromJson, ToJson},
};
use identity_credential::{
    credential::Jwt,
    domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder},
};
use identity_did::DIDUrl;
use identity_iota::{storage::JwkStorage as _, verification::jws::JwsAlgorithm};
use identity_storage::KeyIdStorage;
use identity_storage::{KeyId, KeyType};
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::Sign as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{str::FromStr as _, sync::Arc};
use tracing::{debug, info};

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display, PartialEq)]
pub enum SigningAlgorithm {
    EdDSA,
    ES256,
}

impl Into<Algorithm> for SigningAlgorithm {
    fn into(self) -> Algorithm {
        match self {
            SigningAlgorithm::EdDSA => Algorithm::EdDSA,
            SigningAlgorithm::ES256 => Algorithm::ES256,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Vault {
    Stronghold,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManagedKey {
    #[serde(rename = "id")]
    pub managed_key_id: String,
    pub key_id: String,
    pub alias: String,
    pub signing_algorithm: Option<SigningAlgorithm>,
    pub is_signing_key: bool,
    pub is_removed: bool,
}

#[async_trait]
impl Aggregate for ManagedKey {
    type Command = ManagedKeyCommand;
    type Event = ManagedKeyEvent;
    type Error = ManagedKeyError;
    type Services = Arc<IdentityServices>;

    fn aggregate_type() -> String {
        "managed_key".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ManagedKeyCommand::*;
        use ManagedKeyError::*;
        use ManagedKeyEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            GenerateKey {
                managed_key_id,
                alias,
                signing_algorithm,
            } => {
                let (key_type, alg) = match signing_algorithm {
                    SigningAlgorithm::EdDSA => (KeyType::new("Ed25519"), JwsAlgorithm::EdDSA),
                    SigningAlgorithm::ES256 => (KeyType::new("P256"), JwsAlgorithm::ES256),
                };

                let key_id = services
                    .subject
                    .stronghold_storage
                    .generate(key_type, alg)
                    .await
                    .unwrap()
                    .key_id
                    .to_string();

                Ok(vec![KeyGenerated {
                    managed_key_id,
                    key_id,
                    alias,
                    signing_algorithm,
                }])
            }
            RemoveKey => {
                let key_id = KeyId::new(&self.key_id);

                services.subject.stronghold_storage.delete(&key_id).await.unwrap();
                // FIXME: services.subject.stronghold_storage.delete_key_id().await.unwrap();

                Ok(vec![KeyRemoved {
                    managed_key_id: self.managed_key_id.clone(),
                }])
            }
            UpdateKeyAlias { new_alias } => Ok(vec![KeyAliasUpdated {
                managed_key_id: self.managed_key_id.clone(),
                new_alias,
            }]),
            SetSigningKey => {
                // TODO: Unset other signing keys?
                Ok(vec![SigningKeySet {
                    managed_key_id: self.managed_key_id.clone(),
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ManagedKeyEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            KeyGenerated {
                managed_key_id,
                key_id,
                alias,
                signing_algorithm,
            } => {
                self.managed_key_id = managed_key_id;
                self.key_id = key_id;
                self.alias = alias;
                self.signing_algorithm.replace(signing_algorithm);
            }
            KeyRemoved { managed_key_id } => {
                // Do not reset the entire state so that the removal can be undone if needed?
                // *self = Self::default();
                // self.managed_key_id = managed_key_id;
                self.is_removed = true;
            }
            KeyAliasUpdated {
                managed_key_id: _,
                new_alias,
            } => {
                self.alias = new_alias;
            }
            SigningKeySet { managed_key_id: _ } => {
                self.is_signing_key = true;
            }
        }
    }
}

#[cfg(test)]
pub mod managed_key_tests {
    use super::test_utils::*;
    use super::*;
    use crate::document::aggregate::test_utils::both_verification_methods;
    use agent_shared::config::set_config;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type ManagedKeyTestFramework = TestFramework<ManagedKey>;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use identity_core::{common::Url, convert::FromJson};
    use rstest::*;
    use serde_json::json;
}
