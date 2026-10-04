use crate::catalog::{
    command::CatalogCommand::{self},
    error::CatalogError,
    event::CatalogEvent,
    services::CatalogServices,
};
use chrono::{DateTime, Utc};
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq)]
pub struct Catalog {
    #[serde(rename = "id")]
    pub catalog_id: String,
    pub display: CatalogDisplay,
    pub template_ids: Vec<String>,
    pub visibility: CatalogVisibility,
    pub modified_at: DateTime<Utc>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDisplay {
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, ToSchema)]
pub enum CatalogVisibility {
    Public,
    #[default]
    Private,
}

impl Aggregate for Catalog {
    type Command = CatalogCommand;
    type Event = CatalogEvent;
    type Error = CatalogError;
    type Services = Arc<dyn CatalogServices + 'static>;

    const TYPE: &'static str = "catalog";

    async fn handle(
        &mut self,
        command: Self::Command,
        service: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use CatalogCommand::*;
        use CatalogEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
            CreateCatalog {
                catalog_id,
                display,
                visibility,
            } => {
                if display.name.trim().is_empty() {
                    return Err(CatalogError::MissingCatalogName(
                        "Catalog name cannot be empty".to_string(),
                    ));
                }

                vec![CatalogCreated {
                    id: catalog_id,
                    display,
                    visibility,
                }]
            }
            ChangeCatalogAppearance { catalog_id, display } => {
                if self.deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }

                if display.name.trim().is_empty() {
                    return Err(CatalogError::MissingCatalogName(
                        "Catalog name cannot be empty".to_string(),
                    ));
                }

                vec![CatalogAppearanceChanged {
                    id: catalog_id,
                    display,
                }]
            }
            MakeCatalogPublic { catalog_id } => {
                if self.deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }
                vec![CatalogMadePublic {
                    id: catalog_id,
                    visibility: CatalogVisibility::Public,
                }]
            }
            MakeCatalogPrivate { catalog_id } => {
                if self.deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }
                vec![CatalogMadePrivate {
                    id: catalog_id,
                    visibility: CatalogVisibility::Private,
                }]
            }
            AddTemplateIds {
                catalog_id,
                template_ids,
            } => {
                if self.deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }

                // Check if all template IDs exist before proceeding
                let missing_templates = service.check_all_templates_exist(&template_ids).await;

                if !missing_templates.is_empty() {
                    return Err(CatalogError::TemplateNotFound(missing_templates.join(", ")));
                }

                let new_template_ids: Vec<String> = template_ids
                    .into_iter()
                    .filter(|id| !self.template_ids.contains(id))
                    .collect();

                if new_template_ids.is_empty() {
                    debug!("No new template IDs to add, ignoring AddTemplateIds command");
                    return Ok(());
                }

                vec![TemplateIdsAdded {
                    id: catalog_id,
                    template_ids: new_template_ids,
                }]
            }
            RemoveTemplateIds {
                catalog_id,
                template_ids,
            } => {
                if self.deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }

                let to_remove: Vec<String> = template_ids
                    .into_iter()
                    .filter(|id| self.template_ids.contains(id))
                    .collect();

                if to_remove.is_empty() {
                    debug!("No matching template IDs to remove, ignoring RemoveTemplateIds command");
                    return Ok(());
                }

                vec![TemplateIdsRemoved {
                    id: catalog_id,
                    template_ids: to_remove,
                }]
            }
            DeleteCatalog { catalog_id } => {
                vec![CatalogDeleted { id: catalog_id }]
            }
        };

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use CatalogEvent::*;
        debug!("Applying event: {:?}", event);

        match event {
            CatalogCreated {
                id,
                display,
                visibility,
            } => {
                self.catalog_id = id;
                self.display = display;
                self.visibility = visibility;
                self.modified_at = Utc::now();
            }
            CatalogAppearanceChanged { id: _, display } => {
                self.display = display;
                self.modified_at = Utc::now();
            }
            CatalogMadePublic { id: _, visibility } => {
                self.visibility = visibility;
                self.modified_at = Utc::now();
            }
            CatalogMadePrivate { id: _, visibility } => {
                self.visibility = visibility;
                self.modified_at = Utc::now();
            }
            TemplateIdsAdded { id: _, template_ids } => {
                self.template_ids.extend(template_ids);
                self.modified_at = Utc::now();
            }
            TemplateIdsRemoved { id: _, template_ids } => {
                self.template_ids.retain(|id| !template_ids.contains(id));
                self.modified_at = Utc::now();
            }
            CatalogDeleted { id: _ } => {
                self.deleted = true;
            }
        }
    }
}

#[cfg(test)]
pub mod catalog_tests {
    use super::test_utils::*;
    use super::*;
    use async_trait::async_trait;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;
    use std::sync::Arc;

    pub struct MockCatalogServices {
        pub all_templates_exist: bool,
    }

    impl MockCatalogServices {
        fn successfully_finds_templates() -> Arc<Self> {
            Arc::new(Self {
                all_templates_exist: true,
            })
        }

        fn finds_no_templates() -> Arc<Self> {
            Arc::new(Self {
                all_templates_exist: false,
            })
        }
    }

    #[async_trait]
    impl CatalogServices for MockCatalogServices {
        async fn check_all_templates_exist(&self, ids: &[String]) -> Vec<String> {
            if self.all_templates_exist {
                vec![]
            } else {
                ids.to_vec()
            }
        }
    }

    type CatalogTestFramework = TestFramework<Catalog>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_catalog(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given_no_previous_events()
            .when(CatalogCommand::CreateCatalog {
                catalog_id: catalog_id.clone(),
                display: display.clone(),
                visibility: visibility.clone(),
            })
            .then_expect_events(vec![CatalogEvent::CatalogCreated {
                id: catalog_id,
                display,
                visibility,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_display(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        let new_display = CatalogDisplay {
            name: "Updated Name".to_string(),
            description: "Updated Description".to_string(),
            icon: None,
        };

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::ChangeCatalogAppearance {
                catalog_id: catalog_id.clone(),
                display: new_display.clone(),
            })
            .then_expect_events(vec![CatalogEvent::CatalogAppearanceChanged {
                id: catalog_id,
                display: new_display,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_visibility(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::MakeCatalogPublic {
                catalog_id: catalog_id.clone(),
            })
            .then_expect_events(vec![CatalogEvent::CatalogMadePublic {
                id: catalog_id,
                visibility: CatalogVisibility::Public,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_template_ids(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        let template_ids = ["template-001".to_string(), "template-002".to_string()].to_vec();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::AddTemplateIds {
                catalog_id: catalog_id.clone(),
                template_ids: template_ids.clone(),
            })
            .then_expect_events(vec![CatalogEvent::TemplateIdsAdded {
                id: catalog_id,
                template_ids,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_template_id_already_present(
        catalog_id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    ) {
        let template_ids = ["template-001".to_string(), "template-002".to_string()].to_vec();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![
                CatalogEvent::CatalogCreated {
                    id: catalog_id.clone(),
                    display,
                    visibility,
                },
                CatalogEvent::TemplateIdsAdded {
                    id: catalog_id.clone(),
                    template_ids: template_ids.clone(),
                },
            ])
            .when(CatalogCommand::AddTemplateIds {
                catalog_id,
                template_ids,
            })
            .then_expect_events(vec![])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_template_id_not_found(
        catalog_id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    ) {
        let template_ids = ["nonexistent-template".to_string(), "template-002".to_string()].to_vec();

        CatalogTestFramework::with(MockCatalogServices::finds_no_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::AddTemplateIds {
                catalog_id,
                template_ids: template_ids.clone(),
            })
            .then_expect_error_message(&format!("Template not found: {}", template_ids.join(", ")))
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_remove_template_id(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        let existing_templates = ["template-001".to_string(), "template-002".to_string()].to_vec();
        let to_remove = ["template-001".to_string()].to_vec();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![
                CatalogEvent::CatalogCreated {
                    id: catalog_id.clone(),
                    display,
                    visibility,
                },
                CatalogEvent::TemplateIdsAdded {
                    id: catalog_id.clone(),
                    template_ids: existing_templates.clone(),
                },
            ])
            .when(CatalogCommand::RemoveTemplateIds {
                catalog_id: catalog_id.clone(),
                template_ids: to_remove.clone(),
            })
            .then_expect_events(vec![CatalogEvent::TemplateIdsRemoved {
                id: catalog_id,
                template_ids: to_remove,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_remove_template_id_not_in_catalog(
        catalog_id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    ) {
        let template_ids = ["template-001".to_string()].to_vec();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::RemoveTemplateIds {
                catalog_id,
                template_ids,
            })
            .then_expect_events(vec![])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_delete_catalog(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::DeleteCatalog {
                catalog_id: catalog_id.clone(),
            })
            .then_expect_events(vec![CatalogEvent::CatalogDeleted { id: catalog_id }])
    }
}

#[cfg(any(test, feature = "test_utils"))]
pub mod test_utils {
    use rstest::fixture;

    use super::*;

    #[fixture]
    pub fn catalog_id() -> String {
        "catalog-id-12345".to_string()
    }

    #[fixture]
    pub fn display() -> CatalogDisplay {
        CatalogDisplay {
            name: "Sample Catalog".to_string(),
            description: "A sample Catalog for testing".to_string(),
            icon: None,
        }
    }

    #[fixture]
    pub fn visibility() -> CatalogVisibility {
        CatalogVisibility::Private
    }

    #[fixture]
    pub fn modified_at() -> String {
        "2024-01-01T00:00:00Z".to_string()
    }
}
