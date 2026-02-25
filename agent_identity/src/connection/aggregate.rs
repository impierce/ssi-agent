use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_core::common::{Timestamp, Url};
use identity_did::DIDUrl;
use serde::{de, Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

use crate::services::IdentityServices;

use super::{command::ConnectionCommand, error::ConnectionError, event::ConnectionEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct Connection {
    #[serde(rename = "id")]
    pub connection_id: String,
    // moved alias into the display
    #[schema(value_type = Option<String>)]
    pub domain: Option<Url>,
    #[schema(value_type = Vec<String>)]
    pub dids: Vec<DIDUrl>,
    #[schema(value_type = Option<DisplayProperties>)]
    pub display: Option<DisplayProperties>,
    // TODO: use appropriate value_type for timestamps (also enable crate feature `chrono` or `time`)
    #[schema(value_type = Option<String>)]
    pub first_interacted: Option<Timestamp>,
    // TODO: use appropriate value_type for timestamps (also enable crate feature `chrono` or `time`)
    #[schema(value_type = Option<String>)]
    pub last_interacted: Option<Timestamp>,

    // TODO: How do we want to make distinction between issuer, holder, and verifier capabilities of the `Connection`?
    #[schema(value_type = Option<String>)]
    pub credential_offer_endpoint: Option<Url>,
    // pub issuer_options: Option<IssuerOptions>,
    // pub holder_options: Option<HolderOptions>,
    // pub verifier_options: Option<VerifierOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, utoipa::ToSchema)]
pub struct DisplayProperties {
    #[schema(value_type = Option<String>)]
    pub alias: Option<String>,
    #[schema(value_type = Option<String>)]
    pub locale: Option<String>,
    #[schema(value_type = Option<LogoProperties>)]
    pub logo: Option<LogoProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, utoipa::ToSchema)]
pub struct LogoProperties {
    #[schema(value_type = Option<String>)]
    pub url: Option<Url>,
    #[schema(value_type = Option<String>)]
    pub alt_text: Option<String>,
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
                display,
                domain,
                dids,
                credential_offer_endpoint,
            } => Ok(vec![ConnectionAdded {
                connection_id,
                display,
                domain,
                dids,
                credential_offer_endpoint,
            }]),
            SyncConnection { connection_id } => {
                // todo: fetch and compare the current state of the connection with the actual state of the connection (e.g. by making a request to the domain or credential_offer_endpoint) and emit a ConnectionUpdated event if there are any differences
                Ok(vec![ConnectionUpdated {
                    connection_id,
                    display: self.display.clone(),
                    domain: self.domain.clone(),
                    dids: self.dids.clone(),
                    credential_offer_endpoint: self.credential_offer_endpoint.clone(),
                }])
            }
            RemoveConnection { connection_id } => Ok(vec![ConnectionRemoved { connection_id }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ConnectionEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            ConnectionAdded {
                connection_id,
                display,
                domain,
                dids,
                credential_offer_endpoint,
            } => {
                self.connection_id = connection_id;
                self.display = display;
                self.domain = domain;
                self.dids = dids;
                self.credential_offer_endpoint = credential_offer_endpoint;
            }
            ConnectionUpdated {
                connection_id,
                display,
                domain,
                dids,
                credential_offer_endpoint,
            } => {
                self.connection_id = connection_id;
                self.display = display;
                self.domain = domain;
                self.dids = dids;
                self.credential_offer_endpoint = credential_offer_endpoint;
            }
            ConnectionRemoved { connection_id: _ } => {}
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
        display: DisplayProperties,
        domain: Url,
        dids: Vec<DIDUrl>,
        credential_offer_endpoint: Url,
    ) {
        ConnectionTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ConnectionCommand::AddConnection {
                connection_id: connection_id.clone(),
                display: Some(display.clone()),
                domain: Some(domain.clone()),
                dids: dids.clone(),
                credential_offer_endpoint: Some(credential_offer_endpoint.clone()),
            })
            .then_expect_events(vec![ConnectionEvent::ConnectionAdded {
                connection_id: connection_id.clone(),
                display: Some(display.clone()),
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
    pub fn alias() -> String {
        "My Connection".to_string()
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
