use crate::services::IdentityServices;
use chrono::{DateTime, Utc};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_core::common::Url;
use identity_did::DIDUrl;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

use super::{command::ConnectionCommand, error::ConnectionError, event::ConnectionEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct Connection {
    #[serde(rename = "id")]
    pub connection_id: String,
    pub url: Option<Url>,
    #[schema(value_type = Vec<String>)]
    pub dids: Vec<DIDUrl>,
    pub display: Option<ConnectionDisplayProperties>,
    pub first_interacted_at: Option<DateTime<Utc>>,
    pub last_interacted_at: Option<DateTime<Utc>>,
    // TODO: How do we want to make distinction between issuer, holder, and verifier capabilities of the `Connection`?
    // pub issuer_options: Option<IssuerOptions>,
    // pub holder_options: Option<HolderOptions>,
    // pub verifier_options: Option<VerifierOptions>,
    pub pending_changes: Option<PendingChanges>,
    pub validations: Vec<Validation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub enum Validation {
    DomainLinkage(DomainLinkageValidation),
    LinkedVp(LinkedVpValidation),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct LinkedVpValidation {
    // TODO: add validation!
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct DomainLinkageValidation {
    pub domain: Url,
    pub result: ValidationResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct ValidationResult {
    pub valid: bool,
    pub error: Option<String>,
    pub last_validated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, utoipa::ToSchema)]
pub struct PendingChanges {
    #[schema(value_type = Vec<String>)]
    pub dids: Option<Vec<DIDUrl>>,
    // TODO: Should all changes to the display be notified to a user? Or only changes to the name
    pub display: Option<ConnectionDisplayProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, utoipa::ToSchema)]
pub struct ConnectionDisplayProperties {
    pub name: Option<String>,
    pub locale: Option<String>,
    pub logo: Option<LogoProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, utoipa::ToSchema)]
pub struct LogoProperties {
    pub uri: Option<Url>,
    pub alt_text: Option<String>,
}

impl Aggregate for Connection {
    type Command = ConnectionCommand;
    type Event = ConnectionEvent;
    type Error = ConnectionError;
    type Services = Arc<IdentityServices>;

    const TYPE: &'static str = "connection";

    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use ConnectionCommand::*;
        use ConnectionEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
            AddConnection { connection_id, url } => {
                let metadata = services.fetch_credential_issuer_metadata(&url).await?;
                let connection_display_properties = get_display_from_metadata(metadata.clone());
                let (dids, domain_linkage_valid) = services.fetch_linked_dids(&url).await?;
                let now = services.now();

                let validations = vec![Validation::DomainLinkage(DomainLinkageValidation {
                    domain: url.clone(),
                    result: ValidationResult {
                        valid: domain_linkage_valid,
                        error: None,
                        last_validated_at: now,
                    },
                })];

                Ok(vec![ConnectionAdded {
                    connection_id,
                    display: connection_display_properties,
                    url,
                    dids: dids.clone(),
                    validations,
                    first_interacted_at: Some(now),
                    last_interacted_at: Some(now),
                }])
            }
            _ if self.connection_id.is_empty() => return Err(ConnectionError::ConnectionNotFound),
            SyncConnection { connection_id } => {
                let domain_ref = self
                    .url
                    .as_ref()
                    .ok_or(ConnectionError::MissingDomain(connection_id.clone()))?;

                let metadata = services.fetch_credential_issuer_metadata(domain_ref).await?;
                let new_display = get_display_from_metadata(metadata.clone());
                let (new_dids, domain_linkage_valid) = services.fetch_linked_dids(domain_ref).await?;
                let now = services.now();

                let validations = vec![Validation::DomainLinkage(DomainLinkageValidation {
                    domain: domain_ref.clone(),
                    result: ValidationResult {
                        valid: domain_linkage_valid,
                        error: None,
                        last_validated_at: now,
                    },
                })];

                let proposed = PendingChanges {
                    dids: Some(new_dids),
                    display: new_display,
                };

                let current = PendingChanges {
                    dids: Some(self.dids.clone()),
                    display: self.display.clone(),
                };

                if proposed != current {
                    Ok(vec![ConnectionSynced {
                        connection_id,
                        validations,
                        pending_changes: Some(proposed),
                        last_interacted_at: Some(now),
                    }])
                } else {
                    Ok(vec![ConnectionSynced {
                        connection_id,
                        validations,
                        pending_changes: None,
                        last_interacted_at: Some(now),
                    }])
                }
            }
            AcceptConnectionChanges { connection_id } => match &self.pending_changes {
                Some(pending) => Ok(vec![ConnectionChangesAccepted {
                    connection_id,
                    display: pending.display.clone(),
                    dids: pending.dids.clone().unwrap(),
                    last_interacted_at: Some(services.now()),
                    pending_changes: None,
                }]),
                None => Ok(vec![]),
            },
            RemoveConnection { connection_id } => Ok(vec![ConnectionRemoved { connection_id }]),
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use ConnectionEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            ConnectionAdded {
                connection_id,
                display,
                validations,
                url,
                dids,
                first_interacted_at,
                last_interacted_at,
            } => {
                self.connection_id = connection_id;
                self.display = display;
                self.url = Some(url);
                self.dids = dids;
                self.validations = validations;
                self.first_interacted_at = first_interacted_at;
                self.last_interacted_at = last_interacted_at;
            }
            ConnectionSynced {
                connection_id,
                pending_changes,
                last_interacted_at,
                validations,
            } => {
                self.connection_id = connection_id;
                self.pending_changes = pending_changes;
                self.last_interacted_at = last_interacted_at;
                self.validations = validations;
            }
            ConnectionChangesAccepted {
                connection_id,
                display,
                dids,
                last_interacted_at,
                pending_changes,
            } => {
                self.connection_id = connection_id;
                self.display = display;
                self.dids = dids;
                self.last_interacted_at = last_interacted_at;
                self.pending_changes = pending_changes;
            }
            ConnectionRemoved { connection_id: _ } => self.connection_id = Default::default(),
        }
    }
}

// HELPERS

fn get_display_from_metadata(metadata: CredentialIssuerMetadata) -> Option<ConnectionDisplayProperties> {
    metadata
        .display
        .and_then(|displays: Vec<serde_json::Value>| displays.first().cloned())
        .and_then(|display: serde_json::Value| {
            Some(ConnectionDisplayProperties {
                name: display.get("name")?.as_str().map(String::from),
                locale: display
                    .get("locale")
                    .and_then(|locale| locale.as_str().map(String::from)),
                logo: display.get("logo").map(|logo| LogoProperties {
                    uri: logo.get("uri").and_then(|uri| uri.as_str()?.parse().ok()),
                    alt_text: logo.get("alt_text").and_then(|alt| alt.as_str().map(String::from)),
                }),
            })
        })
}

#[cfg(test)]
pub mod document_tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use cqrs_es::test::TestFramework;
    use tokio::runtime::Runtime;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    type ConnectionTestFramework = TestFramework<Connection>;

    const LINKED_DID_JWT: &str = "eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNI3o2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSJ9.eyJleHAiOjE3NjQ4NzkxMzksImlzcyI6ImRpZDprZXk6ejZNa29USHNnTk5yYnk4SnpDTlExaVJMeVc1UVE2UjhYdXU2QUE4aWdHck1WUFVNIiwibmJmIjoxNjA3MTEyNzM5LCJzdWIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9pZGVudGl0eS5mb3VuZGF0aW9uLy53ZWxsLWtub3duL2RpZC1jb25maWd1cmF0aW9uL3YxIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rb1RIc2dOTnJieThKekNOUTFpUkx5VzVRUTZSOFh1dTZBQThpZ0dyTVZQVU0iLCJvcmlnaW4iOiJpZGVudGl0eS5mb3VuZGF0aW9uIn0sImV4cGlyYXRpb25EYXRlIjoiMjAyNS0xMi0wNFQxNDoxMjoxOS0wNjowMCIsImlzc3VhbmNlRGF0ZSI6IjIwMjAtMTItMDRUMTQ6MTI6MTktMDY6MDAiLCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtvVEhzZ05OcmJ5OEp6Q05RMWlSTHlXNVFRNlI4WHV1NkFBOGlnR3JNVlBVTSIsInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJEb21haW5MaW5rYWdlQ3JlZGVudGlhbCJdfX0.aUFNReA4R5rcX_oYm3sPXqWtso_gjPHnWZsB6pWcGv6m3K8-4JIAvFov3ZTM8HxPOrOL17Qf4vBFdY9oK0HeCQ";
    const TEST_DID: &str = "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM";

    // TODO: Test domain_linkage_validation properly. Currently we exclude these, hence the default to false.
    #[test]
    fn test_add_connection() {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let mock_server = rt.block_on(MockServer::start());

        rt.block_on(async {
            Mock::given(method("GET"))
                .and(path("/.well-known/openid-credential-issuer"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "credential_issuer": mock_server.uri(),
                "credential_endpoint": format!("{}/credentials", mock_server.uri()),
                "display": [
                    {
                        "name": "Time Regulation Institute",
                        "locale": "en",
                        "logo": {
                            "uri": "https://example.com/logo.png",
                            "alt_text": "Organisational Logo"
                        }
                    }
                ],
                "credential_configurations_supported": {}
                    })))
                .mount(&mock_server)
                .await;

            Mock::given(method("GET"))
                .and(path("/.well-known/did-configuration.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@context": "https://identity.foundation/.well-known/did-configuration/v1",
                "linked_dids": [LINKED_DID_JWT]
                    })))
                .mount(&mock_server)
                .await;
        });

        let mock_time = "2026-03-04T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mock_issuer: Url = mock_server.uri().parse().unwrap();
        let services = IdentityServices::default();

        ConnectionTestFramework::with(services)
            .given_no_previous_events()
            .when(ConnectionCommand::AddConnection {
                connection_id: "abcd1234".to_string(),
                url: mock_issuer.clone(),
            })
            .then_expect_events(vec![ConnectionEvent::ConnectionAdded {
                connection_id: "abcd1234".to_string(),
                display: Some(ConnectionDisplayProperties {
                    name: Some("Time Regulation Institute".to_string()),
                    locale: Some("en".to_string()),
                    logo: Some(LogoProperties {
                        uri: Some("https://example.com/logo.png".parse().unwrap()),
                        alt_text: Some("Organisational Logo".to_string()),
                    }),
                }),
                url: mock_issuer.clone(),
                dids: vec![TEST_DID.parse().unwrap()],
                validations: vec![Validation::DomainLinkage(DomainLinkageValidation {
                    domain: mock_issuer,
                    result: ValidationResult {
                        valid: false,
                        error: None,
                        last_validated_at: mock_time,
                    },
                })],
                first_interacted_at: Some(mock_time),
                last_interacted_at: Some(mock_time),
            }]);
    }

    #[test]
    fn test_sync_connection_with_changes() {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let mock_server = rt.block_on(MockServer::start());

        rt.block_on(async {
            Mock::given(method("GET"))
                .and(path("/.well-known/openid-credential-issuer"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "credential_issuer": mock_server.uri(),
                "credential_endpoint": format!("{}/credentials", mock_server.uri()),
                "display": [
                    {
                        "name": "Timeless Institute",
                        "locale": "en",
                        "logo": {
                            "uri": "https://example.com/logo.png",
                            "alt_text": "Organisational Logo"
                        }
                    }
                ],
                "credential_configurations_supported": {}
                    })))
                .mount(&mock_server)
                .await;

            Mock::given(method("GET"))
                .and(path("/.well-known/did-configuration.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@context": "https://identity.foundation/.well-known/did-configuration/v1",
                "linked_dids": [LINKED_DID_JWT]
                    })))
                .mount(&mock_server)
                .await;
        });

        let mock_time = "2026-03-04T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mock_issuer: Url = mock_server.uri().parse().unwrap();
        let services = IdentityServices::default();

        ConnectionTestFramework::with(services)
            .given(vec![ConnectionEvent::ConnectionAdded {
                connection_id: "abcd-123".to_string(),
                display: Some(ConnectionDisplayProperties {
                    name: Some("Time Regulation Institute".to_string()),
                    locale: Some("en".to_string()),
                    logo: Some(LogoProperties {
                        uri: Some("https://example.com/logo.png".parse().unwrap()),
                        alt_text: Some("Organisational Logo".to_string()),
                    }),
                }),
                url: mock_issuer.clone(),
                dids: vec![TEST_DID.parse().unwrap()],
                validations: vec![Validation::DomainLinkage(DomainLinkageValidation {
                    domain: mock_issuer.clone(),
                    result: ValidationResult {
                        valid: false,
                        error: None,
                        last_validated_at: mock_time,
                    },
                })],
                first_interacted_at: Some(mock_time),
                last_interacted_at: Some(mock_time),
            }])
            .when(ConnectionCommand::SyncConnection {
                connection_id: "abcd-123".to_string(),
            })
            .then_expect_events(vec![ConnectionEvent::ConnectionSynced {
                connection_id: "abcd-123".to_string(),
                validations: vec![Validation::DomainLinkage(DomainLinkageValidation {
                    domain: mock_issuer,
                    result: ValidationResult {
                        valid: false,
                        error: None,
                        last_validated_at: mock_time,
                    },
                })],
                pending_changes: Some(PendingChanges {
                    dids: Some(vec![TEST_DID.parse().unwrap()]),
                    display: Some(ConnectionDisplayProperties {
                        name: Some("Timeless Institute".to_string()),
                        locale: Some("en".to_string()),
                        logo: Some(LogoProperties {
                            uri: Some("https://example.com/logo.png".parse().unwrap()),
                            alt_text: Some("Organisational Logo".to_string()),
                        }),
                    }),
                }),
                last_interacted_at: Some(mock_time),
            }]);
    }

    #[test]
    fn accept_connection_changes() {
        let services = IdentityServices::default();
        let mock_time = "2026-03-04T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mock_issuer: Url = "https://example.com".parse().unwrap();

        ConnectionTestFramework::with(services)
            .given(vec![ConnectionEvent::ConnectionSynced {
                connection_id: "abcd1234".to_string(),
                validations: vec![Validation::DomainLinkage(DomainLinkageValidation {
                    domain: mock_issuer,
                    result: ValidationResult {
                        valid: false,
                        error: None,
                        last_validated_at: mock_time,
                    },
                })],
                pending_changes: Some(PendingChanges {
                    dids: Some(vec![TEST_DID.parse().unwrap()]),
                    display: Some(ConnectionDisplayProperties {
                        name: Some("Timeless Institute".to_string()),
                        locale: Some("en".to_string()),
                        logo: Some(LogoProperties {
                            uri: Some("https://example.com/logo.png".parse().unwrap()),
                            alt_text: Some("Organisational Logo".to_string()),
                        }),
                    }),
                }),
                last_interacted_at: Some(mock_time),
            }])
            .when(ConnectionCommand::AcceptConnectionChanges {
                connection_id: "abcd1234".to_string(),
            })
            .then_expect_events(vec![ConnectionEvent::ConnectionChangesAccepted {
                connection_id: "abcd1234".to_string(),
                display: Some(ConnectionDisplayProperties {
                    name: Some("Timeless Institute".to_string()),
                    locale: Some("en".to_string()),
                    logo: Some(LogoProperties {
                        uri: Some("https://example.com/logo.png".parse().unwrap()),
                        alt_text: Some("Organisational Logo".to_string()),
                    }),
                }),
                dids: vec![TEST_DID.parse().unwrap()],
                last_interacted_at: Some(mock_time),
                pending_changes: None,
            }]);
    }

    #[test]
    fn test_remove_connection() {
        let services = IdentityServices::default();
        let mock_time = "2026-03-04T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mock_issuer: Url = "https://example.com".parse().unwrap();

        ConnectionTestFramework::with(services)
            .given(vec![ConnectionEvent::ConnectionAdded {
                connection_id: "abcd1233".to_string(),
                display: Some(ConnectionDisplayProperties {
                    name: Some("Time Regulation Institute".to_string()),
                    locale: Some("en".to_string()),
                    logo: Some(LogoProperties {
                        uri: Some("https://example.com/logo.png".parse().unwrap()),
                        alt_text: Some("Organisational Logo".to_string()),
                    }),
                }),
                url: mock_issuer.clone(),
                dids: vec![TEST_DID.parse().unwrap()],
                validations: vec![Validation::DomainLinkage(DomainLinkageValidation {
                    domain: mock_issuer.clone(),
                    result: ValidationResult {
                        valid: false,
                        error: None,
                        last_validated_at: mock_time,
                    },
                })],
                first_interacted_at: Some(mock_time),
                last_interacted_at: Some(mock_time),
            }])
            .when(ConnectionCommand::RemoveConnection {
                connection_id: "abcd1234".to_string(),
            })
            .then_expect_events(vec![ConnectionEvent::ConnectionRemoved {
                connection_id: "abcd1234".to_string(),
            }]);
    }
}
