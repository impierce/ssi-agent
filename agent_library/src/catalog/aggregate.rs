use crate::catalog::{command::CatalogCommand, error::CatalogError, event::CatalogEvent, services::CatalogServices};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
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
                // TODO! Does a catalog with the same name already exist?
                Ok(vec![CatalogCreated {
                    id: catalog_id,
                    display,
                    visibility,
                }])
            }
            UpdateDisplay { catalog_id, display } => Ok(vec![CatalogDisplayUpdated {
                id: catalog_id,
                display,
            }]),
            UpdateVisibility { catalog_id, visibility } => Ok(vec![VisibilityUpdated {
                id: catalog_id,
                visibility,
            }]),
            AddTemplateId {
                catalog_id,
                template_id,
            } => {
                // TODO! Check if the template is a demo template, should be immutable!
                if !services.template_exists(&template_id).await {
                    return Err(CatalogError::TemplateNotFound(template_id));
                }
                if self.template_ids.contains(&template_id) {
                    debug!(
                        "Template ID {} already in catalog {}, ignoring AddTemplateId command",
                        template_id, catalog_id
                    );
                    return Ok(vec![]);
                }
                Ok(vec![TemplateIdAdded {
                    id: catalog_id,
                    template_id,
                }])
            }
            RemoveTemplateId {
                catalog_id,
                template_id,
            } => {
                if !services.template_exists(&template_id).await {
                    return Err(CatalogError::TemplateNotFound(template_id));
                }
                if !self.template_ids.contains(&template_id) {
                    debug!(
                        "Template ID {} is not part of catalog {}, ignoring RemoveTemplateId command",
                        template_id, catalog_id
                    );
                    return Ok(vec![]);
                }
                Ok(vec![TemplateIdRemoved {
                    id: catalog_id,
                    template_id,
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
            TemplateIdAdded { id: _, template_id } => {
                self.template_ids.push(template_id);
                self.modified_at = Utc::now();
            }
            TemplateIdRemoved { id: _, template_id } => {
                self.template_ids.retain(|id| id != &template_id);
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
    async fn test_add_template_id(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        let template_id = "template-001".to_string();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::AddTemplateId {
                catalog_id: catalog_id.clone(),
                template_id: template_id.clone(),
            })
            .then_expect_events(vec![CatalogEvent::TemplateIdAdded {
                id: catalog_id,
                template_id,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_template_id_already_present(
        catalog_id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    ) {
        let template_id = "template-001".to_string();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![
                CatalogEvent::CatalogCreated {
                    id: catalog_id.clone(),
                    display,
                    visibility,
                },
                CatalogEvent::TemplateIdAdded {
                    id: catalog_id.clone(),
                    template_id: template_id.clone(),
                },
            ])
            .when(CatalogCommand::AddTemplateId {
                catalog_id,
                template_id,
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
        let template_id = "nonexistent-template".to_string();

        CatalogTestFramework::with(MockCatalogServices::finds_no_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::AddTemplateId {
                catalog_id,
                template_id: template_id.clone(),
            })
            .then_expect_error_message(&format!("Template not found: {}", template_id))
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_remove_template_id(catalog_id: String, display: CatalogDisplay, visibility: CatalogVisibility) {
        let template_id = "template-001".to_string();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![
                CatalogEvent::CatalogCreated {
                    id: catalog_id.clone(),
                    display,
                    visibility,
                },
                CatalogEvent::TemplateIdAdded {
                    id: catalog_id.clone(),
                    template_id: template_id.clone(),
                },
            ])
            .when(CatalogCommand::RemoveTemplateId {
                catalog_id: catalog_id.clone(),
                template_id: template_id.clone(),
            })
            .then_expect_events(vec![CatalogEvent::TemplateIdRemoved {
                id: catalog_id,
                template_id,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_remove_template_id_not_in_catalog(
        catalog_id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    ) {
        let template_id = "template-001".to_string();

        CatalogTestFramework::with(MockCatalogServices::successfully_finds_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::RemoveTemplateId {
                catalog_id,
                template_id,
            })
            .then_expect_events(vec![])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_remove_template_id_not_found(
        catalog_id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    ) {
        let template_id = "nonexistent-template".to_string();

        CatalogTestFramework::with(MockCatalogServices::finds_no_templates())
            .given(vec![CatalogEvent::CatalogCreated {
                id: catalog_id.clone(),
                display,
                visibility,
            }])
            .when(CatalogCommand::RemoveTemplateId {
                catalog_id,
                template_id: template_id.clone(),
            })
            .then_expect_error_message(&format!("Template not found: {}", template_id))
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
