use crate::catalog::{command::CatalogCommand, error::CatalogError, event::CatalogEvent, services::CatalogServices};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info};
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, ToSchema)]
pub struct Catalog {
    #[serde(rename = "id")]
    pub catalog_id: String,
    pub display: CatalogDisplay,
    pub template_ids: Vec<String>,
    pub visibility: CatalogVisibility,
    pub modified_at: DateTime<Utc>,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDisplay {
    pub name: String,
    pub description: String,
    pub icon: Option<DisplayIcon>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, ToSchema)]
pub enum CatalogVisibility {
    Public,
    #[default]
    Private,
    Draft,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisplayIcon {
    pub url: String,
    pub alt_text: String,
}

#[async_trait]
impl Aggregate for Catalog {
    type Command = CatalogCommand;
    type Event = CatalogEvent;
    type Error = CatalogError;
    type Services = Arc<dyn CatalogServices>;

    fn aggregate_type() -> String {
        "catalog".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use CatalogCommand::*;
        use CatalogEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateCatalog {
                catalog_id,
                display,
                visibility,
            } => {
                if display.name.trim().is_empty() {
                    return Err(CatalogError::MissingField("Catalog name cannot be empty".to_string()));
                }

                Ok(vec![CatalogCreated {
                    id: catalog_id,
                    display,
                    visibility,
                }])
            }
            UpdateDisplay { catalog_id, display } => {
                if self.is_deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }

                if display.name.trim().is_empty() {
                    return Err(CatalogError::MissingField("Catalog name cannot be empty".to_string()));
                }

                Ok(vec![CatalogDisplayUpdated {
                    id: catalog_id,
                    display,
                }])
            }
            UpdateVisibility { catalog_id, visibility } => {
                if self.is_deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }
                Ok(vec![VisibilityUpdated {
                    id: catalog_id,
                    visibility,
                }])
            }
            AddTemplateIds {
                catalog_id,
                template_ids,
            } => {
                if self.is_deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }

                // Check if all template IDs exist before proceeding
                let missing_templates = services.missing_templates(&template_ids).await;

                if !missing_templates.is_empty() {
                    return Err(CatalogError::TemplatesNotFound(format!(
                        "{}",
                        missing_templates.join(", ").to_string()
                    )));
                }

                let new_template_ids: Vec<String> = template_ids
                    .into_iter()
                    .filter(|id| !self.template_ids.contains(id))
                    .collect();

                let unique_templates: HashSet<_> = new_template_ids.iter().cloned().collect();
                if unique_templates.len() != new_template_ids.len() {
                    return Err(CatalogError::DuplicateTemplate(
                        "Duplicate template IDs found in AddTemplateIds command".to_string(),
                    ));
                }

                if new_template_ids.is_empty() {
                    debug!("No new template IDs to add, ignoring AddTemplateIds command");
                    return Ok(vec![]);
                }

                Ok(vec![TemplateIdsAdded {
                    id: catalog_id,
                    template_ids: new_template_ids,
                }])
            }
            RemoveTemplateIds {
                catalog_id,
                template_ids,
            } => {
                if self.is_deleted {
                    return Err(CatalogError::CatalogNotFound(catalog_id));
                }

                let to_remove: Vec<String> = template_ids
                    .into_iter()
                    .filter(|id| self.template_ids.contains(id))
                    .collect();

                if to_remove.is_empty() {
                    debug!("No matching template IDs to remove, ignoring RemoveTemplateIds command");
                    return Ok(vec![]);
                }

                Ok(vec![TemplateIdsRemoved {
                    id: catalog_id,
                    template_ids: to_remove,
                }])
            }
            DeleteCatalog { catalog_id } => Ok(vec![CatalogDeleted { id: catalog_id }]),
        }
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
            CatalogDisplayUpdated { id: _, display } => {
                self.display = display;
                self.modified_at = Utc::now();
            }
            VisibilityUpdated { id: _, visibility } => {
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
                self.is_deleted = true;
            }
        }
    }
}

#[cfg(test)]
pub mod catalog_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;
    use std::sync::Arc;

    pub struct MockCatalogServices {
        pub template_exists: bool,
    }

    impl MockCatalogServices {
        fn successfully_finds_templates() -> Arc<Self> {
            Arc::new(Self { template_exists: true })
        }

        fn finds_no_templates() -> Arc<Self> {
            Arc::new(Self { template_exists: false })
        }
    }

    #[async_trait]
    impl CatalogServices for MockCatalogServices {
        async fn template_exists(&self, _id: &str) -> bool {
            self.template_exists
        }
        async fn missing_templates(&self, ids: &[String]) -> Vec<String> {
            if self.template_exists {
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
            .when(CatalogCommand::UpdateDisplay {
                catalog_id: catalog_id.clone(),
                display: new_display.clone(),
            })
            .then_expect_events(vec![CatalogEvent::CatalogDisplayUpdated {
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
            .when(CatalogCommand::UpdateVisibility {
                catalog_id: catalog_id.clone(),
                visibility: CatalogVisibility::Public,
            })
            .then_expect_events(vec![CatalogEvent::VisibilityUpdated {
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
            .then_expect_error_message(&format!("Templates not found: {}", template_ids.join(", ")))
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
