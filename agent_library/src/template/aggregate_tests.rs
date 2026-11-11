use std::collections::HashMap;

use cqrs_es::test::TestFramework;
use rstest::rstest;

use super::test_utils::*;
use super::*;

type TemplateTestFramework = TestFramework<Template>;

fn template_created_event_with_status(template_id: &str, status: Status) -> TemplateEvent {
    TemplateEvent::TemplateCreated {
        template_id: template_id.to_string(),
        source_template_id: None,
        title: "Test".to_string(),
        display: Box::new(None),
        data_model: DataModel::W3CVcDataModelV1_1,
        holder_type: HolderType::Individual,
        modified_at: test_utils::modified_at(),
        tags: None,
        status,
        visibility: Visibility::Private,
        credential_expiration: Expiration::default(),
        description: None,
        r#type: vec![],
        schema: Box::new(None),
        schema_properties_attributes: None,
        holder_authorization: Authorization::default(),
        origin: None,
    }
}

#[allow(clippy::too_many_arguments)]
#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template(
    template_id: String,
    title: String,
    display: Option<Display>,
    data_model: DataModel,
    holder_type: HolderType,
    modified_at: String,
    tags: Option<Vec<String>>,
    status: Status,
    visibility: Visibility,
    description: Option<String>,
    r#type: Vec<String>,
    schema: Option<serde_json::Value>,
    schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: title.clone(),
            display: Box::new(display.clone()),
            data_model: data_model.clone(),
            holder_type: holder_type.clone(),
            tags: tags.clone(),
            status: status.clone(),
            visibility: visibility.clone(),
            credential_expiration: None,
            description: description.clone(),
            r#type: r#type.clone(),
            schema: Box::new(schema.clone()),
            schema_properties_attributes: schema_properties_attributes.clone(),
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title,
            display: Box::new(display),
            data_model,
            holder_type,
            modified_at,
            tags,
            status,
            visibility,
            credential_expiration: Expiration::default(),
            description,
            r#type,
            schema: Box::new(schema),
            schema_properties_attributes,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_without_title(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: String::new(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_error_message("A title is required when creating or updating a template")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_empty_title(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_error_message("A title is required when creating or updating a template")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_rejects_archived_status_on_create(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Archived,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_error_message("Invalid status on create: only Draft or Published are allowed")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_title_with_empty_string(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Original".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateTitle {
            template_id,
            title: "   ".to_string(),
        })
        .await
        .then_expect_error_message("A title is required when creating or updating a template")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_display_keeps_empty_name_instead_of_materializing_the_title(template_id: String) {
    let logo = Logo {
        uri: "https://example.com/logo.png".to_string(),
        alt_text: None,
    };

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Diploma".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateDisplay {
            template_id: template_id.clone(),
            display: Display {
                name: String::new(),
                logo: Some(logo.clone()),
            },
        })
        .await
        .then_expect_events(vec![TemplateEvent::DisplayUpdated {
            template_id,
            display: Display {
                name: String::new(),
                logo: Some(logo),
            },
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_title_on_archived_template_is_rejected(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Archived)])
        .when_async(TemplateCommand::UpdateTitle {
            template_id,
            title: "Updated title".to_string(),
        })
        .await
        .then_expect_error_message("Archived templates are immutable except for status changes")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_status_from_published_to_draft_is_rejected(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(
            &template_id,
            Status::Published,
        )])
        .when_async(TemplateCommand::UpdateStatus {
            template_id,
            status: Status::Draft,
        })
        .await
        .then_expect_error_message(
            "Invalid status transition: cannot transition template status from `published` to `draft`",
        )
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_status_from_archived_to_published_is_allowed(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Archived)])
        .when_async(TemplateCommand::UpdateStatus {
            template_id: template_id.clone(),
            status: Status::Published,
        })
        .await
        .then_expect_events(vec![TemplateEvent::StatusUpdated {
            template_id,
            status: Status::Published,
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_tags_normalizes_input(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
        .when_async(TemplateCommand::UpdateTags {
            template_id: template_id.clone(),
            tags: vec![
                " alpha ".to_string(),
                "".to_string(),
                "beta".to_string(),
                "alpha".to_string(),
                "beta ".to_string(),
            ],
        })
        .await
        .then_expect_events(vec![TemplateEvent::TagsUpdated {
            template_id,
            tags: vec!["alpha".to_string(), "beta".to_string()],
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_description_trims_whitespace(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
        .when_async(TemplateCommand::UpdateDescription {
            template_id: template_id.clone(),
            description: "  trimmed description  ".to_string(),
        })
        .await
        .then_expect_events(vec![TemplateEvent::DescriptionUpdated {
            template_id,
            description: "trimmed description".to_string(),
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_type_normalizes_standard_input(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateType {
            template_id: template_id.clone(),
            r#type: vec![
                "ExampleCredential".to_string(),
                "VerifiableCredential".to_string(),
                "ExampleCredential".to_string(),
                "".to_string(),
            ],
        })
        .await
        .then_expect_events(vec![TemplateEvent::TypeUpdated {
            template_id,
            r#type: vec!["VerifiableCredential".to_string(), "ExampleCredential".to_string()],
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_type_rejects_conflicting_open_badges_subtypes(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Open Badges Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
            schema: Box::new(Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "achievement": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" },
                            "criteria": {
                                "type": "object",
                                "properties": {
                                    "narrative": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            }))),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateType {
            template_id,
            r#type: vec![
                "VerifiableCredential".to_string(),
                "OpenBadgeCredential".to_string(),
                "AchievementCredential".to_string(),
            ],
        })
        .await
        .then_expect_error_message(
            "Invalid type: OpenBadges type cannot include both `OpenBadgeCredential` and `AchievementCredential`",
        )
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_credential_expiration_emits_event(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
        .when_async(TemplateCommand::UpdateCredentialExpiration {
            template_id: template_id.clone(),
            credential_expiration: Expiration::Duration("P30D".to_string()),
        })
        .await
        .then_expect_events(vec![TemplateEvent::CredentialExpirationUpdated {
            template_id,
            credential_expiration: Expiration::Duration("P30D".to_string()),
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_credential_expiration_rejects_invalid_iso8601(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
        .when_async(TemplateCommand::UpdateCredentialExpiration {
            template_id,
            credential_expiration: Expiration::Duration("30 days".to_string()),
        })
        .await
        .then_expect_error_message("Invalid expiration value: `30 days` is not a valid ISO 8601 duration")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_extra_types(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "OB with extra types".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![
                "VerifiableCredential".to_string(),
                "OpenBadgeCredential".to_string(),
                "ExtraType".to_string(),
            ],
            schema: Box::new(Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "achievement": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" },
                            "criteria": {
                                "type": "object",
                                "properties": {
                                    "narrative": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            }))),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_error_message("Invalid type: OpenBadges type includes disallowed extra entries: [ExtraType]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_status_reject_invalid_archived_to_draft(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Archived,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateStatus {
            template_id,
            status: Status::Draft,
        })
        .await
        .then_expect_error_message(
            "Invalid status transition: cannot transition template status from `archived` to `draft`",
        )
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_missing_required_achievement_properties(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "OB Missing Props".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![
                "VerifiableCredential".to_string(),
                "OpenBadgeCredential".to_string(),
            ],
            schema: Box::new(Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "achievement": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" }
                        }
                    }
                }
            }))),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message(
            "Missing required OpenBadges 3.0 schema properties: The following required fields must be present in the schema for OpenBadges 3.0 templates: [/achievement/criteria, /achievement/name]"
        )
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_with_array_type_in_schema(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "OB with array type".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![
                "VerifiableCredential".to_string(),
                "OpenBadgeCredential".to_string(),
            ],
            schema: Box::new(Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "achievement": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" },
                            "criteria": {
                                "type": "object",
                                "properties": {
                                    "narrative": { "type": "string" },
                                    "tags": { "type": "array" }
                                }
                            }
                        }
                    }
                }
            }))),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message(
            "Invalid JSON Schema: Array types are not supported in template schemas. Define only object and scalar fields."
        )
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_delete_template_from_published_requires_archive_first(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(
            &template_id,
            Status::Published,
        )])
        .when_async(TemplateCommand::DeleteTemplate { template_id })
        .await
        .then_expect_error_message("Published templates must be archived before they can be deleted")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_delete_template_from_archived_is_allowed(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Archived)])
        .when_async(TemplateCommand::DeleteTemplate {
            template_id: template_id.clone(),
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateDeleted { template_id }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_deleted_template_status_is_terminal(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![
            template_created_event_with_status(&template_id, Status::Draft),
            TemplateEvent::TemplateDeleted {
                template_id: template_id.clone(),
            },
        ])
        .when_async(TemplateCommand::UpdateStatus {
            template_id,
            status: Status::Published,
        })
        .await
        .then_expect_error_message("Deleted templates are terminal and cannot be changed")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_invalid_schema(template_id: String) {
    let invalid_schema = serde_json::json!({
        "type": "not_a_valid_type"
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(invalid_schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid JSON Schema: \"not_a_valid_type\" is not valid under any of the schemas listed in the 'anyOf' keyword")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_no_schema(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_with_invalid_schema(template_id: String) {
    let invalid_schema = serde_json::json!({
        "type": "not_a_valid_type"
    });

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id,
            schema: invalid_schema,
        }).await
        .then_expect_error_message("Invalid JSON Schema: \"not_a_valid_type\" is not valid under any of the schemas listed in the 'anyOf' keyword")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_with_valid_schema(template_id: String) {
    let valid_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id: template_id.clone(),
            schema: valid_schema,
        })
        .await
        .then_expect_events(vec![TemplateEvent::SchemaUpdated {
            template_id,
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                }
            }),
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_invalid_schema_properties_attributes(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "nonexistent".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [nonexistent]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_attributes_but_no_schema(template_id: String) {
    let mut attrs = HashMap::new();
    attrs.insert(
        "name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [name]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_rejects_schema_properties_attributes_for_vc_1_1(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_error_message("schemaPropertiesAttributes are not allowed for W3C VC 1.1 templates")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_field_attributes_with_invalid_keys(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "nonexistent".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id,
            schema_properties_attributes: attrs,
        }).await
        .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [nonexistent]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_field_attributes_with_no_schema(template_id: String) {
    let mut attrs = HashMap::new();
    attrs.insert(
        "name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id,
            schema_properties_attributes: attrs,
        }).await
        .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [name]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_field_attributes_rejected_for_vc_1_1(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id,
            schema_properties_attributes: attrs,
        })
        .await
        .then_expect_error_message("schemaPropertiesAttributes are not allowed for W3C VC 1.1 templates")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_field_attributes_rejects_duplicate_trimmed_keys(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );
    attrs.insert(
        " /name ".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id,
            schema_properties_attributes: attrs,
        })
        .await
        .then_expect_error_message("Duplicate schemaPropertiesAttributes key after trimming: `/name`")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_prunes_attributes(template_id: String) {
    let original_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );
    attrs.insert(
        "/age".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: false,
            r#type: None,
        },
    );

    let new_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut expected_attrs = HashMap::new();
    expected_attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(original_schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id: template_id.clone(),
            schema: new_schema.clone(),
        })
        .await
        .then_expect_events(vec![
            TemplateEvent::SchemaUpdated {
                template_id: template_id.clone(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": { "name": { "type": "string" } }
                }),
                modified_at: test_utils::modified_at(),
            },
            TemplateEvent::SchemaPropertiesAttributesUpdated {
                template_id,
                schema_properties_attributes: expected_attrs,
                modified_at: test_utils::modified_at(),
            },
        ])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_no_prune_needed(template_id: String) {
    let original_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    let new_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        }
    });

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(original_schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id: template_id.clone(),
            schema: new_schema,
        })
        .await
        .then_expect_events(vec![TemplateEvent::SchemaUpdated {
            template_id,
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "age": { "type": "integer" }
                }
            }),
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_rejects_removal_of_immutable_property(template_id: String) {
    // Use a W3C VC template where a field has been manually marked non_removable=true
    // (e.g. by a previous system action). Verify that updating the schema to remove
    // that field is rejected.
    let original_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "description": { "type": "string" }
        },
        "additionalProperties": false
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    attrs.insert(
        "/description".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: false,
            r#type: None,
        },
    );

    // Try to remove the non-removable "/name" property.
    let new_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "description": { "type": "string" }
        }
    });

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(original_schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id,
            schema: new_schema,
        }).await
        .then_expect_error_message("Cannot remove immutable schema properties: The following non-removable properties cannot be removed from the schema: [/name]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_allows_removal_of_non_immutable_property(template_id: String) {
    // OB template with nested schema. Achievement has required fields (non_removable)
    // plus an optional "tag" field (non_removable: false). Removing "tag" must succeed.
    let original_schema = serde_json::json!({
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "achievement": {
                "type": "object",
                "required": ["name", "description", "criteria"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" }
                        },
                        "additionalProperties": false
                    },
                    "tag": { "type": "string" }
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/achievement/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    attrs.insert(
        "/achievement/description".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    attrs.insert(
        "/achievement/criteria/narrative".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    attrs.insert(
        "/achievement/tag".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: false,
            r#type: None,
        },
    );

    // Remove non-removable "/achievement/tag" — should succeed.
    let new_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    let mut expected_attrs = HashMap::new();
    expected_attrs.insert(
        "/achievement/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    expected_attrs.insert(
        "/achievement/description".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    expected_attrs.insert(
        "/achievement/criteria/narrative".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );

    let expected_schema = serde_json::json!({
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "achievement": {
                "type": "object",
                "required": ["criteria", "description", "name"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(original_schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id: template_id.clone(),
            schema: new_schema,
        })
        .await
        .then_expect_events(vec![
            TemplateEvent::SchemaUpdated {
                template_id: template_id.clone(),
                schema: expected_schema,
                modified_at: test_utils::modified_at(),
            },
            TemplateEvent::SchemaPropertiesAttributesUpdated {
                template_id,
                schema_properties_attributes: expected_attrs,
                modified_at: test_utils::modified_at(),
            },
        ])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_errors_when_required_properties_missing(template_id: String) {
    // Nested OB schema that has only description — missing name and criteria/narrative.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "description": { "type": "string" }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Missing required OpenBadges 3.0 schema properties: The following required fields must be present in the schema for OpenBadges 3.0 templates: [/achievement/criteria, /achievement/name]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_succeeds_with_required_properties(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    let mut expected_attrs = HashMap::new();
    expected_attrs.insert(
        "/achievement/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    expected_attrs.insert(
        "/achievement/description".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    expected_attrs.insert(
        "/achievement/criteria/narrative".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );

    let expected_schema = serde_json::json!({
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "achievement": {
                "type": "object",
                "required": ["criteria", "description", "name"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
            schema: Box::new(Some(expected_schema)),
            schema_properties_attributes: Some(expected_attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_allows_profile_object_on_subject_root(template_id: String) {
    // `AchievementSubject` declares `additionalProperties: true`, so UniCore permits a `profile`
    // object on the subject root carrying the recipient's OB 3.0 `Profile` fields. UniCore
    // constrains it to exactly `givenName`/`familyName`/`email`/`dateOfBirth` with enforced types
    // (all strings; `email` → `format: email`, `dateOfBirth` → `format: date`).
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "profile": {
                "type": "object",
                "properties": {
                    "givenName": { "type": "string" },
                    "familyName": { "type": "string" },
                    "email": { "type": "string", "format": "email" },
                    "dateOfBirth": { "type": "string", "format": "date" }
                }
            },
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    let mut expected_attrs = HashMap::new();
    for required_leaf in [
        "/achievement/name",
        "/achievement/description",
        "/achievement/criteria/narrative",
    ] {
        expected_attrs.insert(
            required_leaf.to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
                r#type: None,
            },
        );
    }
    for profile_leaf in [
        "/profile/givenName",
        "/profile/familyName",
        "/profile/email",
        "/profile/dateOfBirth",
    ] {
        expected_attrs.insert(
            profile_leaf.to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: false,
                r#type: None,
            },
        );
    }

    let expected_schema = serde_json::json!({
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "profile": {
                "type": "object",
                "properties": {
                    "givenName": { "type": "string" },
                    "familyName": { "type": "string" },
                    "email": { "type": "string", "format": "email" },
                    "dateOfBirth": { "type": "string", "format": "date" }
                }
            },
            "achievement": {
                "type": "object",
                "required": ["criteria", "description", "name"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
            schema: Box::new(Some(expected_schema)),
            schema_properties_attributes: Some(expected_attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

/// Reusable valid `achievement` block so profile-focused negative tests fail on the profile,
/// not on missing required achievement fields.
fn valid_ob_achievement() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "description": { "type": "string" },
            "criteria": {
                "type": "object",
                "properties": {
                    "narrative": { "type": "string" }
                }
            }
        }
    })
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_unknown_profile_property(template_id: String) {
    // The `profile` object is constrained to the four supported fields; anything else is rejected.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "profile": {
                "type": "object",
                "properties": {
                    "givenName": { "type": "string" },
                    "phoneNumber": { "type": "string" }
                }
            },
            "achievement": valid_ob_achievement()
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Disallowed OpenBadges 3.0 schema properties: The following properties are not allowed for OpenBadges 3.0 templates at path `/profile`: [phoneNumber]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_profile_field_wrong_type(template_id: String) {
    // Profile fields must be strings; a non-string type is rejected.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "profile": {
                "type": "object",
                "properties": {
                    "givenName": { "type": "number" }
                }
            },
            "achievement": valid_ob_achievement()
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid type or format for OpenBadges 3.0 schema properties: The following fields do not match the required type/format: [/profile/givenName]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_profile_email_without_format(template_id: String) {
    // `email` must carry `format: "email"`; a plain string is rejected.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "profile": {
                "type": "object",
                "properties": {
                    "email": { "type": "string" }
                }
            },
            "achievement": valid_ob_achievement()
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid type or format for OpenBadges 3.0 schema properties: The following fields do not match the required type/format: [/profile/email]")
}

#[rstest]
#[case::declared_as_string(serde_json::json!({ "type": "string" }))]
#[case::type_omitted(serde_json::json!({ "properties": { "givenName": { "type": "string" } } }))]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_non_object_profile(
    template_id: String,
    #[case] profile: serde_json::Value,
) {
    // `profile` itself must be an object. Without checking the node's own type, a non-object
    // `profile` would carry no children to validate and slip through untouched.
    // (`"type": "array"` never reaches this check — arrays are rejected for every template
    // schema, see `test_create_open_badges_template_rejects_array_profile`.)
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "profile": profile,
            "achievement": valid_ob_achievement()
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid type or format for OpenBadges 3.0 schema properties: The following fields do not match the required type/format: [/profile]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_array_profile(template_id: String) {
    // An array `profile` is rejected too, but by the schema-wide array rule that runs before
    // OpenBadges validation — hence the different error message.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "profile": { "type": "array", "items": { "type": "string" } },
            "achievement": valid_ob_achievement()
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message(
            "Invalid JSON Schema: Array types are not supported in template schemas. Define only object and scalar fields.",
        )
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_unsanctioned_format_on_profile_name(template_id: String) {
    // The synthetic def is an exact pin: `givenName`/`familyName` are plain strings, so a
    // caller-supplied `format` on them is rejected even though the type is right. `email` keeps
    // the format its def declares and is accepted alongside them.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "profile": {
                "type": "object",
                "properties": {
                    "givenName": { "type": "string", "format": "given-name" },
                    "familyName": { "type": "string", "format": "family-name" },
                    "email": { "type": "string", "format": "email" }
                }
            },
            "achievement": valid_ob_achievement()
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid type or format for OpenBadges 3.0 schema properties: The following fields do not match the required type/format: [/profile/familyName, /profile/givenName]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_succeeds_with_const_required_properties(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "const": "Fixed Achievement Name" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    let mut expected_attrs = HashMap::new();
    expected_attrs.insert(
        "/achievement/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    expected_attrs.insert(
        "/achievement/description".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    expected_attrs.insert(
        "/achievement/criteria/narrative".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );

    let expected_schema = serde_json::json!({
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "achievement": {
                "type": "object",
                "required": ["criteria", "description", "name"],
                "properties": {
                    "name": { "const": "Fixed Achievement Name" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
            schema: Box::new(Some(expected_schema)),
            schema_properties_attributes: Some(expected_attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_attributes_cannot_change_immutable_flag(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let mut existing_attrs = HashMap::new();
    existing_attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );

    // User tries to set non_removable to false — it should be preserved as true.
    let mut user_attrs = HashMap::new();
    user_attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false, // User tries to change this
            r#type: None,
        },
    );

    let mut expected_attrs = HashMap::new();
    expected_attrs.insert(
        "/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: true, // System preserves non_removable
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(existing_attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id: template_id.clone(),
            schema_properties_attributes: user_attrs,
        })
        .await
        .then_expect_events(vec![TemplateEvent::SchemaPropertiesAttributesUpdated {
            template_id,
            schema_properties_attributes: expected_attrs,
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_rejects_disallowed_properties(template_id: String) {
    // A property at the root level that is not in AchievementSubject def.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            },
            "not_allowed_field": { "type": "string" }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Disallowed OpenBadges 3.0 schema properties: The following properties are not allowed for OpenBadges 3.0 templates at path `/`: [not_allowed_field]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_rejects_disallowed_open_badges_properties(template_id: String) {
    let original_schema = serde_json::json!({
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "achievement": {
                "type": "object",
                "required": ["name", "description", "criteria"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" }
                        },
                        "additionalProperties": false
                    }
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/achievement/name".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    attrs.insert(
        "/achievement/description".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );
    attrs.insert(
        "/achievement/criteria/narrative".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: true,
            r#type: None,
        },
    );

    // New schema adds an invalid root-level field.
    let new_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            },
            "invalid_field": { "type": "string" }
        }
    });

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(Some(original_schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id,
            schema: new_schema,
        }).await
        .then_expect_error_message("Disallowed OpenBadges 3.0 schema properties: The following properties are not allowed for OpenBadges 3.0 templates at path `/`: [invalid_field]")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_allows_valid_optional_properties(template_id: String) {
    // Nested OB schema with all required fields plus allowed optional fields.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" },
                            "id": { "type": "string" }
                        }
                    },
                    "image": { "type": "string" },
                    "achievementType": { "type": "string" },
                    "tag": { "type": "string" }
                }
            }
        }
    });

    let required_paths = open_badges_required_leaf_paths();
    let mut expected_attrs = HashMap::new();
    for key in [
        "/achievement/name",
        "/achievement/description",
        "/achievement/criteria/narrative",
        "/achievement/criteria/id",
        "/achievement/image",
        "/achievement/achievementType",
        "/achievement/tag",
    ] {
        expected_attrs.insert(
            key.to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: required_paths.contains(&key.to_string()),
                r#type: None,
            },
        );
    }

    let expected_schema = serde_json::json!({
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "achievement": {
                "type": "object",
                "required": ["criteria", "description", "name"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" },
                            "id": { "type": "string" }
                        }
                    },
                    "image": { "type": "string" },
                    "achievementType": { "type": "string" },
                    "tag": { "type": "string" }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
            schema: Box::new(Some(expected_schema)),
            schema_properties_attributes: Some(expected_attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_open_badges_template_preserves_dollar_schema_keyword(template_id: String) {
    // Callers may include the standard `$schema` meta-schema keyword so their tooling
    // (editors, validators) can validate credential data against the JSON Schema draft
    // before sending it to the API.  The keyword must be accepted and round-tripped.
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "achievement": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    let required_paths = open_badges_required_leaf_paths();
    let mut expected_attrs = HashMap::new();
    for key in [
        "/achievement/name",
        "/achievement/description",
        "/achievement/criteria/narrative",
    ] {
        expected_attrs.insert(
            key.to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: required_paths.contains(&key.to_string()),
                r#type: None,
            },
        );
    }

    let expected_schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["achievement"],
        "properties": {
            "achievement": {
                "type": "object",
                "required": ["criteria", "description", "name"],
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "criteria": {
                        "type": "object",
                        "required": ["narrative"],
                        "properties": {
                            "narrative": { "type": "string" }
                        }
                    }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
            schema: Box::new(Some(expected_schema)),
            schema_properties_attributes: Some(expected_attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

// ── array-type rejection ─────────────────────────────────────────────────────

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_rejects_array_type(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "tags": { "type": "array", "items": { "type": "string" } }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid JSON Schema: Array types are not supported in template schemas. Define only object and scalar fields.")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_schema_rejects_array_type(template_id: String) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "items": { "type": "array", "items": { "type": "string" } }
        }
    });

    TemplateTestFramework::with(())
        .given(vec![TemplateEvent::TemplateCreated {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
            origin: None,
        }])
        .when_async(TemplateCommand::UpdateSchema {
            template_id,
            schema,
        }).await
        .then_expect_error_message("Invalid JSON Schema: Array types are not supported in template schemas. Define only object and scalar fields.")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_nested_schema_and_jp_attribute_key(template_id: String) {
    // A nested schema should be addressable with JSON Pointer attribute keys (leaf only).
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" },
                    "country": { "type": "string" }
                }
            }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/address/city".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );
    attrs.insert(
        "/address/country".to_string(),
        PropertyAttribute {
            selectively_disclosable: false,
            non_removable: false,
            r#type: None,
        },
    );

    // The nested object `address` is stored as provided by the caller.
    let expected_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" },
                    "country": { "type": "string" }
                }
            }
        }
    });

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(attrs.clone()),
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(Some(expected_schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_rejects_attribute_key_pointing_to_object_node(template_id: String) {
    // The intermediate `address` node is not a leaf and must not be addressed.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                }
            }
        }
    });

    let mut attrs = HashMap::new();
    attrs.insert(
        "/address".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: None,
        },
    );

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(attrs),
            holder_authorization: Authorization::default(),
            origin: None,
        }).await
        .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [/address]")
}

#[rstest]
fn property_attribute_type_survives_serde_round_trip() {
    // The `country` field type is not recoverable from standard JSON Schema keywords, so the
    // frontend persists it out-of-band in `schemaPropertiesAttributes[*].type`. Verify it is
    // serialized under the `type` key and deserializes back unchanged.
    let expected = PropertyAttribute {
        selectively_disclosable: false,
        non_removable: false,
        r#type: Some(FormFieldType::Country),
    };

    let json = serde_json::to_value(&expected).unwrap();
    assert_eq!(json["type"], serde_json::json!("country"));
    // An absent `type` must not be emitted, and `non_removable` is not affected.
    assert_eq!(json.get("nonRemovable"), Some(&serde_json::json!(false)));

    let actual: PropertyAttribute = serde_json::from_value(json).unwrap();
    assert_eq!(actual, expected);

    // Omitting `type` on the wire deserializes to `None` (backward compatible).
    let without_type: PropertyAttribute =
        serde_json::from_value(serde_json::json!({ "selectivelyDisclosable": true })).unwrap();
    assert_eq!(without_type.r#type, None);
    assert!(serde_json::to_value(&without_type).unwrap().get("type").is_none());

    // An unrecognized field type is rejected (constrained enum).
    let should_error = serde_json::from_value::<PropertyAttribute>(
        serde_json::json!({ "selectivelyDisclosable": false, "type": "not-a-real-type" }),
    );
    assert!(should_error.is_err());
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_preserves_property_attribute_type(template_id: String) {
    // The `country` field type is caller-supplied metadata the aggregate must carry through
    // untouched: it has to survive command handling and land in the emitted event, alongside the
    // system-controlled flags the aggregate derives itself.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "nationality": { "type": "string" }
        }
    });

    let attributes = HashMap::from([(
        "/nationality".to_string(),
        PropertyAttribute {
            selectively_disclosable: true,
            non_removable: false,
            r#type: Some(FormFieldType::Country),
        },
    )]);

    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(TemplateCommand::CreateNewTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: None,
            description: None,
            r#type: vec![],
            schema: Box::new(Some(schema.clone())),
            schema_properties_attributes: Some(attributes.clone()),
            holder_authorization: Authorization::default(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::TemplateCreated {
            template_id,
            source_template_id: None,
            title: "Test".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: test_utils::modified_at(),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(attributes),
            holder_authorization: Authorization::default(),
            origin: None,
        }])
}

fn origin(digest_byte: char) -> TemplateOrigin {
    TemplateOrigin {
        source: "https://issuer.example".to_string(),
        source_template_id: "0b7c".to_string(),
        digest: format!("sha256-{}", digest_byte.to_string().repeat(64)),
        check_for_updates: true,
    }
}

fn create_command_with_origin(template_id: &str, origin: Option<TemplateOrigin>) -> TemplateCommand {
    TemplateCommand::CreateNewTemplate {
        template_id: template_id.to_string(),
        source_template_id: None,
        title: "Test".to_string(),
        display: Box::new(None),
        data_model: DataModel::W3CVcDataModelV1_1,
        holder_type: HolderType::Individual,
        tags: None,
        status: Status::Draft,
        visibility: Visibility::Private,
        credential_expiration: None,
        description: None,
        r#type: vec![],
        schema: Box::new(None),
        schema_properties_attributes: None,
        holder_authorization: Authorization::default(),
        origin: origin.map(Box::new),
    }
}

fn template_created_event_with_origin(template_id: &str, origin: Option<TemplateOrigin>) -> TemplateEvent {
    let TemplateEvent::TemplateCreated {
        template_id,
        source_template_id,
        title,
        display,
        data_model,
        holder_type,
        modified_at,
        tags,
        status,
        visibility,
        credential_expiration,
        description,
        schema,
        schema_properties_attributes,
        holder_authorization,
        ..
    } = template_created_event_with_status(template_id, Status::Draft)
    else {
        unreachable!()
    };

    TemplateEvent::TemplateCreated {
        template_id,
        source_template_id,
        title,
        display,
        data_model,
        holder_type,
        modified_at,
        tags,
        status,
        visibility,
        credential_expiration,
        description,
        r#type: vec!["VerifiableCredential".to_string()],
        schema,
        schema_properties_attributes,
        holder_authorization,
        origin: origin.map(Box::new),
    }
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_origin(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(create_command_with_origin(&template_id, Some(origin('a'))))
        .await
        .then_expect_events(vec![template_created_event_with_origin(
            &template_id,
            Some(origin('a')),
        )])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_create_template_with_invalid_origin_is_rejected(template_id: String) {
    TemplateTestFramework::with(())
        .given_no_previous_events()
        .when_async(create_command_with_origin(
            &template_id,
            Some(TemplateOrigin {
                digest: "sha256-A".to_string(),
                ..origin('a')
            }),
        ))
        .await
        .then_expect_error_message(
            "Invalid origin: `digest` must be `sha256-` followed by 64 lowercase hexadecimal digits",
        )
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_origin_replaces_origin(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_origin(
            &template_id,
            Some(origin('a')),
        )])
        .when_async(TemplateCommand::UpdateOrigin {
            template_id: template_id.clone(),
            origin: Some(origin('b')),
        })
        .await
        .then_expect_events(vec![TemplateEvent::OriginUpdated {
            template_id,
            origin: Some(origin('b')),
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_origin_turns_update_checks_off_and_on(template_id: String) {
    let opted_out = TemplateOrigin {
        check_for_updates: false,
        ..origin('a')
    };
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_origin(
            &template_id,
            Some(origin('a')),
        )])
        .when_async(TemplateCommand::UpdateOrigin {
            template_id: template_id.clone(),
            origin: Some(opted_out.clone()),
        })
        .await
        .then_expect_events(vec![TemplateEvent::OriginUpdated {
            template_id: template_id.clone(),
            origin: Some(opted_out.clone()),
            modified_at: test_utils::modified_at(),
        }]);

    TemplateTestFramework::with(())
        .given(vec![
            template_created_event_with_origin(&template_id, Some(origin('a'))),
            TemplateEvent::OriginUpdated {
                template_id: template_id.clone(),
                origin: Some(opted_out),
                modified_at: test_utils::modified_at(),
            },
        ])
        .when_async(TemplateCommand::UpdateOrigin {
            template_id: template_id.clone(),
            origin: Some(origin('a')),
        })
        .await
        .then_expect_events(vec![TemplateEvent::OriginUpdated {
            template_id,
            origin: Some(origin('a')),
            modified_at: test_utils::modified_at(),
        }])
}

/// Whether the importer checks for updates is about the copy, not its content, so it never changes the digest.
#[test]
fn update_checks_never_change_the_digest() {
    let mut template = Template {
        origin: Some(origin('a')),
        ..Template::default()
    };
    let digest = template.public_content().digest();

    template.origin = Some(TemplateOrigin {
        check_for_updates: false,
        ..origin('a')
    });

    assert_eq!(template.public_content().digest(), digest);
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_origin_sets_origin_on_template_without_one(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_origin(&template_id, None)])
        .when_async(TemplateCommand::UpdateOrigin {
            template_id: template_id.clone(),
            origin: Some(origin('a')),
        })
        .await
        .then_expect_events(vec![TemplateEvent::OriginUpdated {
            template_id,
            origin: Some(origin('a')),
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_origin_with_none_removes_origin(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_origin(
            &template_id,
            Some(origin('a')),
        )])
        .when_async(TemplateCommand::UpdateOrigin {
            template_id: template_id.clone(),
            origin: None,
        })
        .await
        .then_expect_events(vec![TemplateEvent::OriginUpdated {
            template_id,
            origin: None,
            modified_at: test_utils::modified_at(),
        }])
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_origin_with_invalid_origin_is_rejected(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_origin(&template_id, None)])
        .when_async(TemplateCommand::UpdateOrigin {
            template_id,
            origin: Some(TemplateOrigin {
                source: "issuer.example".to_string(),
                ..origin('a')
            }),
        })
        .await
        .then_expect_error_message("Invalid origin: `source` must be an absolute URI")
}

#[rstest]
#[serial_test::serial]
#[tokio::test]
async fn test_update_origin_on_archived_template_is_rejected(template_id: String) {
    TemplateTestFramework::with(())
        .given(vec![template_created_event_with_status(&template_id, Status::Archived)])
        .when_async(TemplateCommand::UpdateOrigin {
            template_id,
            origin: Some(origin('a')),
        })
        .await
        .then_expect_error_message("Archived templates are immutable except for status changes")
}

#[rstest]
fn origin_events_apply_to_aggregate_and_view(template_id: String) {
    use crate::template::views::TemplateView;
    use cqrs_es::{EventEnvelope, View};

    let events = [
        template_created_event_with_origin(&template_id, Some(origin('a'))),
        TemplateEvent::OriginUpdated {
            template_id: template_id.clone(),
            origin: Some(origin('b')),
            modified_at: "2024-02-01T00:00:00Z".to_string(),
        },
    ];

    let mut aggregate = Template::default();
    let mut view = TemplateView::default();
    for (sequence, event) in (1..).zip(events) {
        view.update(&EventEnvelope {
            aggregate_id: template_id.clone(),
            sequence,
            payload: event.clone(),
            metadata: HashMap::new(),
        });
        aggregate.apply(event);
    }

    assert_eq!(aggregate.origin, Some(origin('b')));
    assert_eq!(view.origin, Some(origin('b')));
    assert_eq!(view.modified_at.as_deref(), Some("2024-02-01T00:00:00Z"));

    aggregate.apply(TemplateEvent::OriginUpdated {
        template_id,
        origin: None,
        modified_at: "2024-03-01T00:00:00Z".to_string(),
    });

    assert_eq!(aggregate.origin, None);
}

#[rstest]
fn template_created_event_without_origin_keeps_its_stored_form(template_id: String) {
    let event = template_created_event_with_origin(&template_id, None);
    let mut stored = serde_json::to_value(&event).unwrap();

    assert!(stored["TemplateCreated"].get("origin").is_none(), "{stored}");
    assert_eq!(serde_json::from_value::<TemplateEvent>(stored.clone()).unwrap(), event);

    stored["TemplateCreated"]["origin"] = serde_json::to_value(origin('a')).unwrap();
    assert_eq!(
        serde_json::from_value::<TemplateEvent>(stored).unwrap(),
        template_created_event_with_origin(&template_id, Some(origin('a')))
    );
}

#[test]
fn template_view_stored_without_origin_deserializes() {
    let view: Template = serde_json::from_value(serde_json::json!({
        "id": "template_id",
        "title": "Test",
        "data_model": "w3c_vc_data_model_v2-0",
        "holder_type": "individual",
        "status": "draft",
        "visibility": "private",
        "credential_expiration": { "type": "never" },
        "type": ["VerifiableCredential"],
        "schema": null,
        "holder_authorization": { "pre_authorized": true },
    }))
    .unwrap();

    assert_eq!(view.origin, None);
}

#[test]
fn delete_template_command_does_not_deserialize_as_update_origin() {
    let command: TemplateCommand = serde_json::from_value(serde_json::json!({ "template_id": "template_id" })).unwrap();

    assert!(matches!(command, TemplateCommand::DeleteTemplate { .. }), "{command:?}");

    let command: TemplateCommand =
        serde_json::from_value(serde_json::json!({ "template_id": "template_id", "origin": null })).unwrap();

    assert!(
        matches!(command, TemplateCommand::UpdateOrigin { origin: None, .. }),
        "{command:?}"
    );
}

/// A Draft W3C VCDM 2.0 template with one `name` claim, created with `description` and `schema_properties_attributes`.
fn create_command(
    template_id: &str,
    description: Option<&str>,
    schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
) -> TemplateCommand {
    TemplateCommand::CreateNewTemplate {
        template_id: template_id.to_string(),
        source_template_id: None,
        title: "Test".to_string(),
        display: Box::new(None),
        data_model: DataModel::W3CVcDataModelV2_0,
        holder_type: HolderType::Individual,
        tags: None,
        status: Status::Draft,
        visibility: Visibility::Private,
        credential_expiration: None,
        description: description.map(ToString::to_string),
        r#type: vec![],
        schema: Box::new(Some(serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        }))),
        schema_properties_attributes,
        holder_authorization: Authorization::default(),
        origin: None,
    }
}

fn disclosable() -> PropertyAttribute {
    PropertyAttribute {
        selectively_disclosable: true,
        non_removable: false,
        r#type: None,
    }
}

/// The template after `commands`, each handled against the events the ones before it emitted.
async fn template_after(commands: Vec<TemplateCommand>) -> Template {
    let mut events = Vec::new();
    for command in commands {
        let emitted = TemplateTestFramework::with(())
            .given(events.clone())
            .when_async(command)
            .await
            .inspect_result()
            .unwrap();
        events.extend(emitted);
    }

    let mut template = Template::default();
    for event in events {
        template.apply(event);
    }
    template
}

/// A copy created with a description, and one whose description is updated to the same input, store and digest the
/// same, so an imported copy updated by a re-import digests like its source.
#[rstest]
#[case::padded("  padded description  ", Some("padded description"))]
#[case::empty("", None)]
#[case::blank("   ", None)]
#[serial_test::serial]
#[tokio::test]
async fn test_create_and_update_store_a_description_alike(
    template_id: String,
    #[case] description: &str,
    #[case] expected: Option<&str>,
) {
    let created = template_after(vec![create_command(&template_id, Some(description), None)]).await;
    let updated = template_after(vec![
        create_command(&template_id, None, None),
        TemplateCommand::UpdateDescription {
            template_id: template_id.clone(),
            description: description.to_string(),
        },
    ])
    .await;

    assert_eq!(created.description.as_deref(), expected);
    assert_eq!(updated.description.as_deref(), expected);
    assert_eq!(created.public_content().digest(), updated.public_content().digest());
}

/// Attribute keys are trimmed on create as on update, and no attributes are stored as `None` either way.
#[rstest]
#[case::padded_key(HashMap::from([(" /name ".to_string(), disclosable())]), Some(HashMap::from([("/name".to_string(), disclosable())])))]
#[case::empty(HashMap::new(), None)]
#[serial_test::serial]
#[tokio::test]
async fn test_create_and_update_store_schema_properties_attributes_alike(
    template_id: String,
    #[case] attributes: HashMap<String, PropertyAttribute>,
    #[case] expected: Option<HashMap<String, PropertyAttribute>>,
) {
    let created = template_after(vec![create_command(&template_id, None, Some(attributes.clone()))]).await;
    let updated = template_after(vec![
        create_command(&template_id, None, None),
        TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id: template_id.clone(),
            schema_properties_attributes: attributes,
        },
    ])
    .await;

    assert_eq!(created.schema_properties_attributes, expected);
    assert_eq!(updated.schema_properties_attributes, expected);
    assert_eq!(created.public_content().digest(), updated.public_content().digest());
}
