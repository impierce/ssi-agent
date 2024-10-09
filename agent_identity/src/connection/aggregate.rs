use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_core::common::{Timestamp, Url};
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::services::IdentityServices;

use super::{command::ConnectionCommand, error::ConnectionError, event::ConnectionEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Connection {
    pub connection_id: String,
    pub domain: Option<Url>,
    pub dids: Vec<DIDUrl>,
    pub first_interacted: Option<Timestamp>,
    pub last_interacted: Option<Timestamp>,

    // TODO: How do we want to make distinction between issuer, holder, and verifier capabilities of the `Connection`?
    pub credential_offer_endpoint: Option<Url>,
    // pub issuer_options: Option<IssuerOptions>,
    // pub holder_options: Option<HolderOptions>,
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

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use ConnectionCommand::*;
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

    #[rstest]
    #[serial_test::serial]
    async fn test_add_connection(
        connection_id: String,
        domain: Url,
        dids: Vec<DIDUrl>,
        credential_offer_endpoint: Url,
    ) {
        ConnectionTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ConnectionCommand::AddConnection {
                connection_id: connection_id.clone(),
                domain: Some(domain.clone()),
                dids: dids.clone(),
                credential_offer_endpoint: Some(credential_offer_endpoint.clone()),
            })
            .then_expect_events(vec![ConnectionEvent::ConnectionAdded {
                connection_id: connection_id.clone(),
                domain: Some(domain.clone()),
                dids: dids.clone(),
                credential_offer_endpoint: Some(credential_offer_endpoint.clone()),
            }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use identity_core::common::Url;
    use identity_did::DIDUrl;
    use rstest::fixture;

    #[fixture]
    pub fn connection_id() -> String {
        "connection_id".to_string()
    }

    #[fixture]
    pub fn domain() -> Url {
        "http://example.org".parse().unwrap()
    }

    #[fixture]
    pub fn dids() -> Vec<DIDUrl> {
        vec!["did:example:123".parse().unwrap()]
    }

    #[fixture]
    pub fn credential_offer_endpoint() -> Url {
        "http://example.org/openid4vci/offers".parse().unwrap()
    }
}
