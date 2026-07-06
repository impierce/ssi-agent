use std::collections::HashMap;

pub use super::aggregate::{DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Visibility};
use agent_shared::config::Authorization;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

// TODO: Add `modified_at` to metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum TemplateEvent {
    TemplateCreated {
        template_id: String,
        source_template_id: Option<String>,
        title: String,
        display: Box<Option<Display>>,
        data_model: DataModel,
        holder_type: HolderType,
        modified_at: String,
        tags: Option<Vec<String>>,
        status: Status,
        visibility: Visibility,
        credential_expiration: Expiration,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Box<Option<serde_json::Value>>,
        schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
        holder_authorization: Authorization,
    },
    TitleUpdated {
        template_id: String,
        title: String,
        modified_at: String,
    },
    DisplayUpdated {
        template_id: String,
        display: Display,
        modified_at: String,
    },
    TagsUpdated {
        template_id: String,
        tags: Vec<String>,
        modified_at: String,
    },
    StatusUpdated {
        template_id: String,
        status: Status,
        modified_at: String,
    },
    VisibilityUpdated {
        template_id: String,
        visibility: Visibility,
        modified_at: String,
    },
    DescriptionUpdated {
        template_id: String,
        description: String,
        modified_at: String,
    },
    TypeUpdated {
        template_id: String,
        r#type: Vec<String>,
        modified_at: String,
    },
    SchemaUpdated {
        template_id: String,
        schema: serde_json::Value,
        modified_at: String,
    },
    SchemaPropertiesAttributesUpdated {
        template_id: String,
        schema_properties_attributes: HashMap<String, PropertyAttribute>,
        modified_at: String,
    },
    CredentialExpirationUpdated {
        template_id: String,
        credential_expiration: Expiration,
        modified_at: String,
    },
    HolderAuthorizationUpdated {
        template_id: String,
        holder_authorization: Authorization,
        modified_at: String,
    },
    TemplateDeleted {
        template_id: String,
    },
}

impl DomainEvent for TemplateEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}

/// Wire-format regression tests: every variant is round-tripped through JSON and checked
/// against a checked-in "golden" JSON literal. If a golden fixture stops matching, either the
/// change is breaking (bump `event_version` and add an upcaster, see `docs/event-versioning.md`)
/// or the fixture needs deliberate updating.
#[cfg(test)]
mod wire_format_tests {
    use super::*;
    use crate::template::aggregate::Logo;
    use serde_json::json;

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: TemplateEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: TemplateEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: TemplateEvent =
            serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(from_golden, event, "golden fixture no longer deserializes into the expected event");
    }

    #[test]
    fn template_created() {
        let mut schema_properties_attributes = HashMap::new();
        schema_properties_attributes.insert(
            "/name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                non_removable: false,
            },
        );

        let event = TemplateEvent::TemplateCreated {
            template_id: "template-id".to_string(),
            source_template_id: None,
            title: "Sample Template".to_string(),
            display: Box::new(Some(Display {
                name: "Sample Display".to_string(),
                logo: Some(Logo {
                    uri: "https://example.com/logo.png".to_string(),
                    alt_text: Some("Logo".to_string()),
                }),
            })),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
            tags: Some(vec!["tag1".to_string(), "tag2".to_string()]),
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::Duration("P90D".to_string()),
            description: Some("Sample description".to_string()),
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(Some(json!({
                "type": "object",
                "properties": { "name": { "type": "string" } }
            }))),
            schema_properties_attributes: Some(schema_properties_attributes),
            holder_authorization: Authorization {
                pre_authorized: true,
                tx_code_constraints: None,
            },
        };
        let golden = json!({
            "TemplateCreated": {
                "template_id": "template-id",
                "source_template_id": null,
                "title": "Sample Template",
                "display": {
                    "name": "Sample Display",
                    "logo": { "uri": "https://example.com/logo.png", "alt_text": "Logo" }
                },
                "data_model": "w3c_vc_data_model_v2-0",
                "holder_type": "individual",
                "modified_at": "2024-01-01T00:00:00Z",
                "tags": ["tag1", "tag2"],
                "status": "draft",
                "visibility": "private",
                "credential_expiration": { "type": "duration", "value": "P90D" },
                "description": "Sample description",
                "type": ["VerifiableCredential"],
                "schema": {
                    "type": "object",
                    "properties": { "name": { "type": "string" } }
                },
                "schema_properties_attributes": {
                    "/name": { "selectivelyDisclosable": true, "nonRemovable": false }
                },
                "holder_authorization": { "pre_authorized": true }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn title_updated() {
        let event = TemplateEvent::TitleUpdated {
            template_id: "template-id".to_string(),
            title: "New Title".to_string(),
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "TitleUpdated": {
                "template_id": "template-id",
                "title": "New Title",
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn display_updated() {
        let event = TemplateEvent::DisplayUpdated {
            template_id: "template-id".to_string(),
            display: Display {
                name: "Sample Display".to_string(),
                logo: None,
            },
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "DisplayUpdated": {
                "template_id": "template-id",
                "display": { "name": "Sample Display" },
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn tags_updated() {
        let event = TemplateEvent::TagsUpdated {
            template_id: "template-id".to_string(),
            tags: vec!["tag1".to_string()],
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "TagsUpdated": {
                "template_id": "template-id",
                "tags": ["tag1"],
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn status_updated() {
        let event = TemplateEvent::StatusUpdated {
            template_id: "template-id".to_string(),
            status: Status::Archived,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "StatusUpdated": {
                "template_id": "template-id",
                "status": "archived",
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn visibility_updated() {
        let event = TemplateEvent::VisibilityUpdated {
            template_id: "template-id".to_string(),
            visibility: Visibility::Public,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "VisibilityUpdated": {
                "template_id": "template-id",
                "visibility": "public",
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn description_updated() {
        let event = TemplateEvent::DescriptionUpdated {
            template_id: "template-id".to_string(),
            description: "New description".to_string(),
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "DescriptionUpdated": {
                "template_id": "template-id",
                "description": "New description",
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn type_updated() {
        let event = TemplateEvent::TypeUpdated {
            template_id: "template-id".to_string(),
            r#type: vec!["VerifiableCredential".to_string(), "CustomType".to_string()],
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "TypeUpdated": {
                "template_id": "template-id",
                "type": ["VerifiableCredential", "CustomType"],
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn schema_updated() {
        let event = TemplateEvent::SchemaUpdated {
            template_id: "template-id".to_string(),
            schema: json!({ "type": "object", "properties": { "name": { "type": "string" } } }),
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "SchemaUpdated": {
                "template_id": "template-id",
                "schema": { "type": "object", "properties": { "name": { "type": "string" } } },
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn schema_properties_attributes_updated() {
        let mut schema_properties_attributes = HashMap::new();
        schema_properties_attributes.insert(
            "/name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                non_removable: true,
            },
        );

        let event = TemplateEvent::SchemaPropertiesAttributesUpdated {
            template_id: "template-id".to_string(),
            schema_properties_attributes: schema_properties_attributes.clone(),
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "SchemaPropertiesAttributesUpdated": {
                "template_id": "template-id",
                "schema_properties_attributes": {
                    "/name": { "selectivelyDisclosable": true, "nonRemovable": true }
                },
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_expiration_updated() {
        let event = TemplateEvent::CredentialExpirationUpdated {
            template_id: "template-id".to_string(),
            credential_expiration: Expiration::Never,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "CredentialExpirationUpdated": {
                "template_id": "template-id",
                "credential_expiration": { "type": "never" },
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn holder_authorization_updated() {
        let event = TemplateEvent::HolderAuthorizationUpdated {
            template_id: "template-id".to_string(),
            holder_authorization: Authorization {
                pre_authorized: false,
                tx_code_constraints: None,
            },
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let golden = json!({
            "HolderAuthorizationUpdated": {
                "template_id": "template-id",
                "holder_authorization": { "pre_authorized": false },
                "modified_at": "2024-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn template_deleted() {
        let event = TemplateEvent::TemplateDeleted {
            template_id: "template-id".to_string(),
        };
        let golden = json!({
            "TemplateDeleted": {
                "template_id": "template-id"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
