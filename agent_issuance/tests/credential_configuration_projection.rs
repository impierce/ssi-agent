use agent_issuance::application::credential_configuration_projection::CredentialConfigurationProjection;
use agent_issuance::services::IssuanceServices;
use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{DataModel, Display, Status, Visibility};
use agent_library::template::command::TemplateCommand;
use agent_library::template::event::{Expiration, HolderType, TemplateEvent};
use agent_secret_manager::service::Service;
use agent_shared::handlers::{command_handler, query_handler};
use agent_store::in_memory::InMemory;
use agent_store::{issuance_state, library_state};
use cqrs_es::{EventEnvelope, Query};
use std::collections::HashMap;
use std::sync::Arc;

fn create_test_event_template_created(
    template_id: &str,
    types: Vec<String>,
    data_model: DataModel,
    title: String,
    display: Option<Display>,
    status: Status,
) -> EventEnvelope<agent_library::template::aggregate::Template> {
    EventEnvelope {
        aggregate_id: template_id.to_string(),
        sequence: 1,
        payload: TemplateEvent::TemplateCreated {
            template_id: template_id.to_string(),
            source_template_id: None,
            title,
            display: Box::new(display),
            data_model,
            creator: None,
            holder_type: HolderType::Individual,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
            tags: None,
            status,
            visibility: Visibility::Private,
            expiration: Expiration::Never,
            description: None,
            r#type: types,
            schema: Box::new(None),
            schema_properties_attributes: None,
        },
        metadata: HashMap::new(),
    }
}

async fn setup() -> (Arc<IssuanceState>, Arc<LibraryState>, CredentialConfigurationProjection) {
    let issuance = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
    let (projection, template_view_handle) = CredentialConfigurationProjection::new(issuance.clone());
    // Build the library state WITHOUT the projection (the projection dispatches commands to issuance_state,
    // not to this library state). Then wire the real view repo into the projection's OnceLock handle so
    // that partial-update re-queries use the same MemRepository that the CQRS framework updates.
    let lib_state = Arc::new(library_state(&InMemory, Default::default(), vec![]).await);
    assert!(
        template_view_handle.set(lib_state.query.template.clone()).is_ok(),
        "template view already initialized"
    );
    (issuance, lib_state, projection)
}

#[tokio::test]
async fn test_template_created_registers_credential_configuration() {
    let (issuance, _library, projection) = setup().await;

    let template_id = "my-template";
    let event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "My Credential".to_string(),
        None,
        Status::Published,
    );

    projection.dispatch(template_id, &[event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    assert!(server_config.credential_configurations.contains_key(template_id));
}

#[tokio::test]
async fn test_template_created_with_v2_data_model_uses_vc_sd_jwt_format() {
    let (issuance, _library, projection) = setup().await;

    let template_id = "v2-template";
    let event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "My Credential".to_string(),
        None,
        Status::Published,
    );

    projection.dispatch(template_id, &[event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    let (_, config_obj, _) = server_config.credential_configurations.get(template_id).unwrap();
    assert!(matches!(
        &config_obj.credential_format,
        oid4vci::credential_format_profiles::CredentialFormats::VcSdJwt(_)
    ));
}

#[tokio::test]
async fn test_template_created_with_draft_status_skips_registration() {
    let (issuance, _library, projection) = setup().await;

    let template_id = "draft-template";
    let event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "Draft Credential".to_string(),
        None,
        Status::Draft,
    );

    projection.dispatch(template_id, &[event]).await;

    let contains_key = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .map(|sc| sc.credential_configurations.contains_key(template_id))
        .unwrap_or(false);
    assert!(!contains_key);
}

#[tokio::test]
async fn test_template_created_with_deleted_status_skips_registration() {
    let (issuance, _library, projection) = setup().await;

    let template_id = "deleted-template";
    let event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "My Credential".to_string(),
        None,
        Status::Deleted,
    );

    projection.dispatch(template_id, &[event]).await;

    // A ServerConfig may not exist yet; if it does, it must not contain the deleted template.
    let contains_key = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .map(|sc| sc.credential_configurations.contains_key(template_id))
        .unwrap_or(false);
    assert!(!contains_key);
}

#[tokio::test]
async fn test_display_updated_reflects_in_credential_configuration() {
    let (issuance, lib_state, projection) = setup().await;

    let template_id = "display-update-template";

    // Create the template in the shared library state with an initial display.
    command_handler(
        template_id,
        &lib_state.command.template,
        TemplateCommand::CreateTemplate {
            template_id: template_id.to_string(),
            source_template_id: None,
            title: "Display Update Test".to_string(),
            display: Box::new(Some(Display {
                name: "Original Display".to_string(),
                logo: None,
            })),
            data_model: DataModel::W3CVcDataModelV2_0,
            creator: None,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Published,
            visibility: Visibility::Private,
            expiration: None,
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
        },
    )
    .await
    .unwrap();

    // Register the initial credential configuration via TemplateCreated.
    let create_event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "My Credential".to_string(),
        Some(Display {
            name: "Original Display".to_string(),
            logo: None,
        }),
        Status::Published,
    );
    projection.dispatch(template_id, &[create_event]).await;

    // Update the display in the shared library state (the projection will re-query this exact view).
    command_handler(
        template_id,
        &lib_state.command.template,
        TemplateCommand::UpdateDisplay {
            template_id: template_id.to_string(),
            display: Display {
                name: "Updated Display".to_string(),
                logo: None,
            },
        },
    )
    .await
    .unwrap();

    // Dispatch DisplayUpdated — the projection re-queries the shared view and picks up the new display.
    let update_event: EventEnvelope<agent_library::template::aggregate::Template> = EventEnvelope {
        aggregate_id: template_id.to_string(),
        sequence: 2,
        payload: TemplateEvent::DisplayUpdated {
            template_id: template_id.to_string(),
            display: Display {
                name: "Updated Display".to_string(),
                logo: None,
            },
            modified_at: "2024-01-01T00:01:00Z".to_string(),
        },
        metadata: HashMap::new(),
    };
    projection.dispatch(template_id, &[update_event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    let (_, config_obj, _) = server_config.credential_configurations.get(template_id).unwrap();
    let display_name = config_obj
        .credential_metadata
        .as_ref()
        .unwrap()
        .display
        .as_ref()
        .unwrap()
        .first()
        .unwrap()
        .name
        .clone();
    assert_eq!(display_name, "Updated Display");
}

#[tokio::test]
async fn test_title_updated_while_in_draft_skips_sync() {
    let (issuance, library_for_query, projection) = setup().await;

    let template_id = "draft-update-template";

    // Create the template in Draft status in the library state.
    command_handler(
        template_id,
        &library_for_query.command.template,
        TemplateCommand::CreateTemplate {
            template_id: template_id.to_string(),
            source_template_id: None,
            title: "Draft Title".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            creator: None,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            expiration: None,
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
        },
    )
    .await
    .unwrap();

    // Dispatch TitleUpdated — template is still Draft, so no credential config should be created.
    let update_event: EventEnvelope<agent_library::template::aggregate::Template> = EventEnvelope {
        aggregate_id: template_id.to_string(),
        sequence: 2,
        payload: TemplateEvent::TitleUpdated {
            template_id: template_id.to_string(),
            title: "New Draft Title".to_string(),
            modified_at: "2024-01-01T00:01:00Z".to_string(),
        },
        metadata: HashMap::new(),
    };
    projection.dispatch(template_id, &[update_event]).await;

    let contains_key = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .map(|sc| sc.credential_configurations.contains_key(template_id))
        .unwrap_or(false);
    assert!(!contains_key);
}

#[tokio::test]
async fn test_title_updated_refreshes_credential_configuration() {
    let (issuance, library_for_query, projection) = setup().await;

    let template_id = "updatable-template";

    // Pre-populate the library state that the projection re-queries on partial updates.
    command_handler(
        template_id,
        &library_for_query.command.template,
        TemplateCommand::CreateTemplate {
            template_id: template_id.to_string(),
            source_template_id: None,
            title: "Original Title".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            creator: None,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Published,
            visibility: Visibility::Private,
            expiration: None,
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
        },
    )
    .await
    .unwrap();

    // Dispatch TemplateCreated to register the initial credential configuration.
    let create_event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "Original Title".to_string(),
        None,
        Status::Published,
    );
    projection.dispatch(template_id, &[create_event]).await;

    // Apply the title update to the library state that the projection will re-query.
    command_handler(
        template_id,
        &library_for_query.command.template,
        TemplateCommand::UpdateTitle {
            template_id: template_id.to_string(),
            title: "Updated Title".to_string(),
        },
    )
    .await
    .unwrap();

    // Dispatch TitleUpdated so the projection re-queries and syncs the credential configuration.
    let update_event: EventEnvelope<agent_library::template::aggregate::Template> = EventEnvelope {
        aggregate_id: template_id.to_string(),
        sequence: 2,
        payload: TemplateEvent::TitleUpdated {
            template_id: template_id.to_string(),
            title: "Updated Title".to_string(),
            modified_at: "2024-01-01T00:01:00Z".to_string(),
        },
        metadata: HashMap::new(),
    };
    projection.dispatch(template_id, &[update_event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    let (_, config_obj, _) = server_config.credential_configurations.get(template_id).unwrap();
    let display_name = config_obj
        .credential_metadata
        .as_ref()
        .unwrap()
        .display
        .as_ref()
        .unwrap()
        .first()
        .unwrap()
        .name
        .clone();
    assert_eq!(display_name, "Updated Title");
}

#[tokio::test]
async fn test_template_deleted_removes_credential_configuration() {
    let (issuance, _library, projection) = setup().await;

    let template_id = "to-be-deleted";

    // Register the credential configuration first.
    let create_event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "Temp".to_string(),
        None,
        Status::Published,
    );
    projection.dispatch(template_id, &[create_event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    assert!(server_config.credential_configurations.contains_key(template_id));

    // Now delete it.
    let delete_event: EventEnvelope<agent_library::template::aggregate::Template> = EventEnvelope {
        aggregate_id: template_id.to_string(),
        sequence: 2,
        payload: TemplateEvent::TemplateDeleted {
            template_id: template_id.to_string(),
        },
        metadata: HashMap::new(),
    };
    projection.dispatch(template_id, &[delete_event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    assert!(!server_config.credential_configurations.contains_key(template_id));
}

#[tokio::test]
async fn test_status_updated_to_published_creates_credential_configuration() {
    let (issuance, lib_state, projection) = setup().await;

    let template_id = "draft-to-published";

    // Create template in Draft — no credential config should be registered yet.
    command_handler(
        template_id,
        &lib_state.command.template,
        TemplateCommand::CreateTemplate {
            template_id: template_id.to_string(),
            source_template_id: None,
            title: "My Credential".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            creator: None,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            expiration: None,
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
        },
    )
    .await
    .unwrap();

    let create_event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "My Credential".to_string(),
        None,
        Status::Draft,
    );
    projection.dispatch(template_id, &[create_event]).await;

    let contains_key = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .map(|sc| sc.credential_configurations.contains_key(template_id))
        .unwrap_or(false);
    assert!(
        !contains_key,
        "Draft template should not have a credential configuration"
    );

    // Transition to Published in the shared library state.
    command_handler(
        template_id,
        &lib_state.command.template,
        TemplateCommand::UpdateStatus {
            template_id: template_id.to_string(),
            status: Status::Published,
        },
    )
    .await
    .unwrap();

    // Dispatch StatusUpdated — the projection should now create the credential configuration.
    let status_event: EventEnvelope<agent_library::template::aggregate::Template> = EventEnvelope {
        aggregate_id: template_id.to_string(),
        sequence: 2,
        payload: TemplateEvent::StatusUpdated {
            template_id: template_id.to_string(),
            status: Status::Published,
            modified_at: "2024-01-01T00:01:00Z".to_string(),
        },
        metadata: HashMap::new(),
    };
    projection.dispatch(template_id, &[status_event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    assert!(
        server_config.credential_configurations.contains_key(template_id),
        "Published template should have a credential configuration"
    );
}

#[tokio::test]
async fn test_status_updated_to_deleted_removes_credential_configuration() {
    let (issuance, lib_state, projection) = setup().await;

    let template_id = "published-to-deleted";

    // Create and publish the template.
    command_handler(
        template_id,
        &lib_state.command.template,
        TemplateCommand::CreateTemplate {
            template_id: template_id.to_string(),
            source_template_id: None,
            title: "Temp".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            creator: None,
            holder_type: HolderType::Individual,
            tags: None,
            status: Status::Published,
            visibility: Visibility::Private,
            expiration: None,
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
        },
    )
    .await
    .unwrap();

    let create_event = create_test_event_template_created(
        template_id,
        vec!["VerifiableCredential".to_string()],
        DataModel::W3CVcDataModelV2_0,
        "Temp".to_string(),
        None,
        Status::Published,
    );
    projection.dispatch(template_id, &[create_event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    assert!(server_config.credential_configurations.contains_key(template_id));

    // Soft-delete via StatusUpdated.
    let status_event: EventEnvelope<agent_library::template::aggregate::Template> = EventEnvelope {
        aggregate_id: template_id.to_string(),
        sequence: 2,
        payload: TemplateEvent::StatusUpdated {
            template_id: template_id.to_string(),
            status: Status::Deleted,
            modified_at: "2024-01-01T00:01:00Z".to_string(),
        },
        metadata: HashMap::new(),
    };
    projection.dispatch(template_id, &[status_event]).await;

    let server_config = query_handler(SERVER_CONFIG_ID, &issuance.query.server_config)
        .await
        .unwrap()
        .unwrap();
    assert!(!server_config.credential_configurations.contains_key(template_id));
}
