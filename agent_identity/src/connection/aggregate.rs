use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_core::common::{Timestamp, Url};
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::services::IdentityServices;

use super::{command::ConnectionCommand, error::ConnectionError, event::ConnectionEvent};

// #[derive(Debug, Clone, Serialize, Deserialize, Default)]
// pub struct HolderOptions {
//     pub credential_offer_endpoint: Option<Url>,
// }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Connection {
    pub connection_id: String,
    pub domain: Option<Url>,
    pub dids: Vec<DIDUrl>,
    pub first_interacted: Option<Timestamp>,
    pub last_interacted: Option<Timestamp>,
    // // TBD:
    // pub issuer_options: Option<IssuerOptions>,
    // pub holder_options: Option<HolderOptions>,
    pub credential_offer_endpoint: Option<Url>,
    // pub verifier_options: Option<VerifierOptions>,
}

#[async_trait]
impl Aggregate for Connection {
    type Command = ConnectionCommand;
    type Event = ConnectionEvent;
    type Error = ConnectionError;
    type Services = Arc<IdentityServices>;

    fn aggregate_type() -> String {
        "connection".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ConnectionCommand::*;
        use ConnectionError::*;
        use ConnectionEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            AddConnection {
                connection_id,
                domain,
                dids,
                credential_offer_endpoint,
            } => Ok(vec![ConnectionAdded {
                connection_id,
                domain,
                dids,
                credential_offer_endpoint,
            }]),
            AddDomain { connection_id, domain } => Ok(vec![DomainAdded { connection_id, domain }]),
            AddDid { connection_id, did } => Ok(vec![DidAdded { connection_id, did }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ConnectionEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            ConnectionAdded {
                connection_id,
                domain,
                dids,
                credential_offer_endpoint,
            } => {
                self.connection_id = connection_id;
                self.domain = domain;
                self.dids = dids;
                self.credential_offer_endpoint = credential_offer_endpoint;
            }
            DomainAdded { domain, .. } => {
                self.domain.replace(domain);
            }
            DidAdded { did, .. } => {
                self.dids.push(did);
            }
        }
    }
}

#[cfg(test)]
pub mod document_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type ConnectionTestFramework = TestFramework<Connection>;

    // #[rstest]
    // #[serial_test::serial]
    // async fn test_add_connection() {
    //     ConnectionTestFramework::with(IdentityServices::default())
    //         .given_no_previous_events()
    //         .when(ConnectionCommand::AddConnection {})
    //         .then_expect_events(vec![ConnectionEvent::ConnectionAdded {}])
    // }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {}
