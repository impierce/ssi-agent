use crate::catalogue::{
    command::CatalogueCommand, error::CatalogueError, event::CatalogueEvent, services::CatalogueServices,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq)]
pub struct Catalogue {
    #[serde(rename = "id")]
    pub catalogue_id: String,
    pub display: CatalogueDisplay,
    pub template_ids: Vec<String>,
    pub visibility: CatalogueVisibility,
    pub modified_at: DateTime<Utc>,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq)]
pub struct CatalogueDisplay {
    pub name: String,
    pub description: String,
    pub icon: Option<DisplayIcon>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum CatalogueVisibility {
    Public,
    #[default]
    Private,
    Draft,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Deserialize)]
pub struct DisplayIcon {
    pub url: String,
    pub alt_text: String,
}

#[async_trait]
impl Aggregate for Catalogue {
    type Command = CatalogueCommand;
    type Event = CatalogueEvent;
    type Error = CatalogueError;
    type Services = Arc<dyn CatalogueServices>;

    fn aggregate_type() -> String {
        "catalogue".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use CatalogueCommand::*;
        use CatalogueEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateCatalogue {
                catalogue_id,
                display,
                visibility,
            } => {
                // Todo! Does a catalogue with the same name already exist?
                Ok(vec![CatalogueCreated {
                    id: catalogue_id,
                    display,
                    visibility,
                }])
            }
            UpdateCatalogueDisplay { catalogue_id, display } => Ok(vec![CatalogueDisplayUpdated {
                id: catalogue_id,
                display,
            }]),
            UpdateVisibility {
                catalogue_id,
                visibility,
            } => Ok(vec![VisibilityUpdated {
                id: catalogue_id,
                visibility,
            }]),
            AddTemplateId {
                catalogue_id,
                template_id,
            } => {
                if !services.template_exists(&template_id).await {
                    return Err(CatalogueError::TemplateNotFound(template_id));
                }
                if self.template_ids.contains(&template_id) {
                    debug!(
                        "Template ID {} already in catalogue {}, ignoring AddTemplateId command",
                        template_id, catalogue_id
                    );
                    return Ok(vec![]);
                }
                Ok(vec![TemplateIdAdded {
                    id: catalogue_id,
                    template_id,
                }])
            }
            RemoveTemplateId {
                catalogue_id,
                template_id,
            } => {
                if !services.template_exists(&template_id).await {
                    return Err(CatalogueError::TemplateNotFound(template_id));
                }
                if !self.template_ids.contains(&template_id) {
                    debug!(
                        "Template ID {} is not part of catalogue {}, ignoring RemoveTemplateId command",
                        template_id, catalogue_id
                    );
                    return Ok(vec![]);
                }
                Ok(vec![TemplateIdRemoved {
                    id: catalogue_id,
                    template_id,
                }])
            }
            DeleteCatalogue { catalogue_id } => Ok(vec![CatalogueDeleted { id: catalogue_id }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CatalogueEvent::*;
        debug!("Applying event: {:?}", event);

        match event {
            CatalogueCreated {
                id,
                display,
                visibility,
            } => {
                self.catalogue_id = id;
                self.display = display;
                self.visibility = visibility;
                self.modified_at = Utc::now();
            }
            CatalogueDisplayUpdated { id: _, display } => {
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
            CatalogueDeleted { id: _ } => {
                self.is_deleted = true;
            }
        }
    }
}
