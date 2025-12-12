use super::{command::ManagedKeyCommand, error::ManagedKeyError, event::ManagedKeyEvent};
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
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::Sign as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{str::FromStr as _, sync::Arc};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManagedKey {
    #[serde(rename = "id")]
    pub managed_key_id: String,
    pub alias: Option<String>,
    pub key_id: Option<String>,
    pub signing_algorithm: Option<String>,
}

#[async_trait]
impl Aggregate for ManagedKey {
    type Command = ManagedKeyCommand;
    type Event = ManagedKeyEvent;
    type Error = ManagedKeyError;
    type Services = ();

    fn aggregate_type() -> String {
        "managed_key".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ManagedKeyCommand::*;
        use ManagedKeyError::*;
        use ManagedKeyEvent::*;

        info!("Handling command: {:?}", command);

        match command {}
    }

    fn apply(&mut self, event: Self::Event) {
        use ManagedKeyEvent::*;

        debug!("Applying event: {:?}", event);

        match event {}
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
