use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::authorization_request::Object;
use serde::{Deserialize, Serialize};
use siopv2::siopv2::SIOPv2;
use std::{sync::Arc, vec};
use tracing::info;

use crate::services::VerificationServices;

use super::{command::ConnectionCommand, error::ConnectionError, event::ConnectionEvent};

pub type SIOPv2AuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<SIOPv2>>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Connection {
    // TODO: Does user data need to be stored in UniCore at all?
    id_token: String,
}

#[async_trait]
impl Aggregate for Connection {
    type Command = ConnectionCommand;
    type Event = ConnectionEvent;
    type Error = ConnectionError;
    type Services = Arc<VerificationServices>;

    fn aggregate_type() -> String {
        "connection".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ConnectionCommand::*;
        use ConnectionEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            VerifySIOPv2AuthorizationResponse {
                // TODO: use this once `RelyingPartyManager` uses the official SIOPv2 validation logic.
                siopv2_authorization_request: _,
                siopv2_authorization_response,
            } => {
                let relying_party = &services.relying_party;

                let _ = relying_party
                    .validate_response(&siopv2_authorization_response)
                    .await
                    .map_err(|e| ConnectionError::InvalidSIOPv2AuthorizationResponse(e))?;

                let id_token = siopv2_authorization_response.extension.id_token.clone();

                Ok(vec![SIOPv2AuthorizationResponseVerified {
                    id_token: id_token.clone(),
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ConnectionEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            SIOPv2AuthorizationResponseVerified { id_token } => {
                self.id_token = id_token;
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;

    use agent_shared::secret_manager::secret_manager;
    use cqrs_es::test::TestFramework;
    use lazy_static::lazy_static;
    use oid4vc_core::authorization_response::AuthorizationResponse;
    use oid4vc_manager::ProviderManager;

    use crate::authorization_request::aggregate::tests::SIOPV2_AUTHORIZATION_REQUEST;
    use crate::services::test_utils::test_verification_services;

    use super::*;

    type ConnectionTestFramework = TestFramework<Connection>;

    #[test]
    #[serial_test::serial]
    fn test_verify_siopv2_authorization_response() {
        let verification_services = test_verification_services();

        ConnectionTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(ConnectionCommand::VerifySIOPv2AuthorizationResponse {
                siopv2_authorization_request: SIOPV2_AUTHORIZATION_REQUEST.clone(),
                siopv2_authorization_response: SIOPV2_AUTHORIZATION_RESPONSE.clone(),
            })
            .then_expect_events(vec![ConnectionEvent::SIOPv2AuthorizationResponseVerified {
                id_token: ID_TOKEN.clone(),
            }]);
    }

    lazy_static! {
        static ref SIOPV2_AUTHORIZATION_RESPONSE: AuthorizationResponse<SIOPv2> = {
            let provider_manager =
                ProviderManager::new([Arc::new(futures::executor::block_on(async { secret_manager().await }))])
                    .unwrap();
            provider_manager
                .generate_response(&SIOPV2_AUTHORIZATION_REQUEST, Default::default())
                .unwrap()
        };
        static ref ID_TOKEN: String = SIOPV2_AUTHORIZATION_RESPONSE.extension.id_token.clone();
    }
}
