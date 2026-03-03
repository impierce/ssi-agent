use crate::services::IdentityServices;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
use identity_core::common::Url;
use identity_did::DIDUrl;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info};

use super::{command::ConnectionCommand, error::ConnectionError, event::ConnectionEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct Connection {
    #[serde(rename = "id")]
    pub connection_id: String,
    #[schema(value_type = Option<String>)]
    pub domain: Option<Url>,
    #[schema(value_type = Vec<String>)]
    pub dids: Vec<DIDUrl>,
    #[schema(value_type = Option<DisplayProperties>)]
    pub display: Option<DisplayProperties>,
    // TODO: use appropriate value_type for timestamps (also enable crate feature `chrono` or `time`)
    #[schema(value_type = Option<String>)]
    pub first_interacted: Option<DateTime<Utc>>,
    // TODO: use appropriate value_type for timestamps (also enable crate feature `chrono` or `time`)
    #[schema(value_type = Option<String>)]
    pub last_interacted: Option<DateTime<Utc>>,

    // TODO: How do we want to make distinction between issuer, holder, and verifier capabilities of the `Connection`?
    #[schema(value_type = Option<String>)]
    pub credential_offer_endpoint: Option<Url>,
    // pub issuer_options: Option<IssuerOptions>,
    // pub holder_options: Option<HolderOptions>,
    // pub verifier_options: Option<VerifierOptions>,
    pub pending_changes: Option<ConnectionProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, utoipa::ToSchema)]
pub struct ConnectionProperties {
    #[serde(rename = "id")]
    pub connection_id: String,
    #[schema(value_type = Option<String>)]
    pub domain: Option<Url>,
    #[schema(value_type = Vec<String>)]
    pub dids: Vec<DIDUrl>,
    #[schema(value_type = Option<DisplayProperties>)]
    pub display: Option<DisplayProperties>,
    #[schema(value_type = Option<String>)]
    pub first_interacted: Option<DateTime<Utc>>,
    #[schema(value_type = Option<String>)]
    pub last_interacted: Option<DateTime<Utc>>,
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

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ConnectionCommand::*;
        use ConnectionEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            AddConnection { connection_id, domain } => {
                let domain_ref = domain
                    .as_ref()
                    .ok_or(ConnectionError::MissingDomain(connection_id.clone()))?;

                let metadata = services.fetch_credential_issuer_metadata(domain_ref).await?;
                let display_properties = get_display_from_metadata(metadata.clone());
                let supported_did_methods = get_did_methods_from_metadata(metadata);

                let now = Utc::now();

                // IMPORTANT TODO: DID Configuration
                let mut dids: Vec<DIDUrl> = Vec::new();
                for method in &supported_did_methods {
                    match method.as_str() {
                        "did:web" => {
                            let did = services.resolve_did_web(domain_ref).await?;
                            dids.push(did);
                        }
                        "did:iota" => {
                            // TODO: implement did:iota resolution
                        }
                        _ => {}
                    }
                }

                Ok(vec![ConnectionAdded {
                    connection_id,
                    display: display_properties,
                    domain,
                    dids: dids.clone(),
                    first_interacted: Some(now),
                    last_interacted: Some(now),
                }])
            }
            SyncConnection { connection_id } => {
                let domain = self
                    .credential_offer_endpoint
                    .as_ref()
                    .ok_or(ConnectionError::MissingCredentialOfferEndpoint)?;

                let metadata = services.fetch_credential_issuer_metadata(&domain).await?;
                let new_display_properties = get_display_from_metadata(metadata);

                if new_display_properties != self.display {
                    let proposed_changes = ConnectionProperties {
                        connection_id: connection_id.clone(),
                        domain: self.domain.clone(),
                        dids: self.dids.clone(),
                        display: new_display_properties,
                        first_interacted: self.first_interacted,
                        last_interacted: self.last_interacted,
                    };
                    // Right now we are only checking this! TODO: Add more checks.
                    Ok(vec![ConnectionSynced {
                        pending_changes: Some(proposed_changes),
                        last_interacted: Some(Utc::now()),
                    }])
                } else {
                    Ok(vec![])
                }
            }
            AcceptConnectionChanges { connection_id } => {
                let pending = self
                    .pending_changes
                    .as_ref()
                    .ok_or(ConnectionError::ConnectionSyncFailed(
                        "Failed to Accept Pending Changes".to_string(),
                    ))?;

                Ok(vec![ConnectionChangesAccepted {
                    connection_id,
                    display: pending.display.clone(),
                    domain: pending.domain.clone(),
                    dids: pending.dids.clone(),
                    last_interacted: Some(Utc::now()),
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
                first_interacted,
                last_interacted,
            } => {
                self.connection_id = connection_id;
                self.display = display;
                self.domain = domain;
                self.dids = dids;
                self.first_interacted = first_interacted;
                self.last_interacted = last_interacted;
            }
            ConnectionSynced {
                pending_changes,
                last_interacted,
            } => {
                self.pending_changes = pending_changes;
                self.last_interacted = last_interacted;
            }
            ConnectionChangesAccepted {
                connection_id,
                display,
                domain,
                dids,
                last_interacted,
            } => {
                self.connection_id = connection_id;
                self.display = display;
                self.domain = domain;
                self.dids = dids;
                self.last_interacted = last_interacted;
                self.pending_changes = None;
            }
            ConnectionRemoved { connection_id: _ } => {}
        }
    }
}

// HELPER

pub fn get_display_from_metadata(metadata: CredentialIssuerMetadata) -> Option<DisplayProperties> {
    metadata
        .display
        .and_then(|displays: Vec<serde_json::Value>| displays.first().cloned())
        .and_then(|display: serde_json::Value| {
            Some(DisplayProperties {
                alias: display.get("name")?.as_str().map(String::from),
                locale: display
                    .get("locale")
                    .and_then(|locale| locale.as_str().map(String::from)),
                logo: display.get("logo").and_then(|logo| {
                    Some(LogoProperties {
                        url: logo.get("uri").and_then(|uri| uri.as_str()?.parse().ok()),
                        alt_text: logo.get("alt_text").and_then(|alt| alt.as_str().map(String::from)),
                    })
                }),
            })
        })
}

pub fn get_did_methods_from_metadata(metadata: CredentialIssuerMetadata) -> Vec<String> {
    metadata
        .credential_configurations_supported
        .values()
        .flat_map(|configs| configs.cryptographic_binding_methods_supported.clone())
        .collect::<HashSet<String>>()
        .into_iter()
        .collect()
}

// #[cfg(test)]
// pub mod document_tests {
//     use super::test_utils::*;
//     use super::*;
//     use cqrs_es::test::TestFramework;
//     use rstest::rstest;

//     type ConnectionTestFramework = TestFramework<Connection>;

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_add_connection(
//         connection_id: String,
//         domain: Url,
//         credential_offer_endpoint: Url,
//     ) {
//         ConnectionTestFramework::with(IdentityServices::default())
//             .given_no_previous_events()
//             .when(ConnectionCommand::AddConnection {
//                 connection_id: connection_id.clone(),
//                 display: Some(display.clone()),
//                 domain: Some(domain.clone()),
//                 dids: dids.clone(),
//                 credential_offer_endpoint: Some(credential_offer_endpoint.clone()),
//             })
//             .then_expect_events(vec![ConnectionEvent::ConnectionAdded {
//                 connection_id: connection_id.clone(),
//                 display: Some(display.clone()),
//                 domain: Some(domain.clone()),
//                 dids: dids.clone(),
//                 credential_offer_endpoint: Some(credential_offer_endpoint.clone()),
//             }])
//     }
// }

// #[cfg(feature = "test_utils")]
// pub mod test_utils {
//     use super::DisplayProperties;
//     use identity_core::common::Url;
//     use identity_did::DIDUrl;
//     use rstest::fixture;

//     #[fixture]
//     pub fn connection_id() -> String {
//         "connection_id".to_string()
//     }

//     #[fixture]
//     pub fn display() -> DisplayProperties {
//         DisplayProperties {
//             alias: Some("The Cool Organisation".to_string()),
//             locale: None,
//             logo: None,
//         }
//     }

//     #[fixture]
//     pub fn domain() -> Url {
//         "http://example.org".parse().unwrap()
//     }

//     #[fixture]
//     pub fn dids() -> Vec<DIDUrl> {
//         vec!["did:example:123".parse().unwrap()]
//     }

//     #[fixture]
//     pub fn credential_offer_endpoint() -> Url {
//         "http://example.org/openid4vci/offers".parse().unwrap()
//     }
// }
