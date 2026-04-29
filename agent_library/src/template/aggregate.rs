use std::collections::HashMap;

use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tracing::{debug, info};

use super::{command::TemplateCommand, error::TemplateError, event::TemplateEvent};

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, utoipa::ToSchema)]
pub struct Logo {
    pub uri: String,
    pub alt_text: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, utoipa::ToSchema)]
pub struct Display {
    pub name: String,
    pub logo: Option<Logo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, utoipa::ToSchema)]
pub enum DataModel {
    // See https://www.w3.org/TR/vc-data-model-1.1/
    #[serde(rename = "w3c_vc_data_model_v1-1")]
    W3CVcDataModelV1_1,
    // See https://www.w3.org/TR/vc-data-model-2.0/
    #[serde(rename = "w3c_vc_data_model_v2-0")]
    W3CVcDataModelV2_0,
    // See https://www.imsglobal.org/spec/ob/v3p0/
    #[serde(rename = "open_badges_3-0")]
    OpenBadges3_0,
    // See https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model
    #[serde(rename = "european_learning_model_v3-3")]
    EuropeanLearningModelV3_3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HolderType {
    Individual,
    Organization,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = TemplateStatus)]
pub enum Status {
    #[default]
    Draft,
    Published,
    Archived,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    #[default]
    Private,
    Public,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PropertyAttribute {
    selectively_disclosable: bool,
    /// Whether this property is immutable (cannot be removed or renamed from the schema).
    /// Determined by the data model and cannot be altered through any command.
    /// For OpenBadges 3.0 templates, all standard-mandated properties are immutable.
    /// Defaults to `false`.
    #[serde(default)]
    immutable: bool,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Template {
    #[serde(rename = "id")]
    pub template_id: String,
    pub source_template_id: Option<String>,
    pub title: Option<String>,
    pub display: Option<Display>,
    pub data_model: Option<DataModel>,
    pub creator: Option<String>,
    pub holder_type: Option<HolderType>,
    pub modified_at: Option<String>,
    pub tags: Vec<String>,
    pub status: Status,
    pub visibility: Visibility,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Box<Option<serde_json::Value>>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
}

#[async_trait]
impl Aggregate for Template {
    type Command = TemplateCommand;
    type Event = TemplateEvent;
    type Error = TemplateError;
    type Services = ();

    fn aggregate_type() -> String {
        "template".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use TemplateCommand::*;
        use TemplateEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateTemplate {
                template_id,
                source_template_id,
                title,
                display,
                data_model,
                creator,
                holder_type,
                tags,
                status,
                visibility,
                description,
                r#type,
                schema,
                schema_properties_attributes,
            } => {
                if let Some(ref s) = *schema {
                    validate_json_schema(s)?;
                }

                if let Some(ref attrs) = schema_properties_attributes {
                    validate_schema_properties_attributes(&schema, attrs)?;
                }

                // For OpenBadges 3.0 templates, auto-populate immutable attributes
                // for all schema properties. The `immutable` flag is system-determined
                // by the data model and cannot be altered through any command.
                let schema_properties_attributes = if data_model == Some(DataModel::OpenBadges3_0) {
                    if let Some(ref s) = *schema {
                        let property_keys = get_schema_property_keys(s);
                        let mut attrs = schema_properties_attributes.unwrap_or_default();
                        for key in property_keys {
                            attrs.entry(key).or_insert(PropertyAttribute {
                                selectively_disclosable: false,
                                immutable: true,
                            });
                        }
                        // Ensure immutable is always true for OpenBadges properties,
                        // even if user-provided attributes tried to set it differently.
                        for key in get_schema_property_keys(s) {
                            if let Some(attr) = attrs.get_mut(&key) {
                                attr.immutable = true;
                            }
                        }
                        Some(attrs)
                    } else {
                        schema_properties_attributes
                    }
                } else {
                    schema_properties_attributes
                };

                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TemplateCreated {
                    template_id,
                    source_template_id,
                    title,
                    display,
                    data_model,
                    creator,
                    holder_type,
                    modified_at,
                    tags,
                    status,
                    visibility,
                    description,
                    r#type,
                    schema,
                    schema_properties_attributes,
                }])
            }
            UpdateTitle { template_id, title } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TitleUpdated {
                    template_id,
                    title,
                    modified_at,
                }])
            }
            UpdateDisplay { template_id, display } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![DisplayUpdated {
                    template_id,
                    display,
                    modified_at,
                }])
            }
            UpdateDataModel {
                template_id,
                data_model,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![DataModelUpdated {
                    template_id,
                    data_model,
                    modified_at,
                }])
            }
            UpdateCreator { template_id, creator } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![CreatorUpdated {
                    template_id,
                    creator,
                    modified_at,
                }])
            }
            UpdateHolderType {
                template_id,
                holder_type,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![HolderTypeUpdated {
                    template_id,
                    holder_type,
                    modified_at,
                }])
            }
            UpdateTags { template_id, tags } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TagsUpdated {
                    template_id,
                    tags,
                    modified_at,
                }])
            }
            UpdateStatus { template_id, status } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![StatusUpdated {
                    template_id,
                    status,
                    modified_at,
                }])
            }
            UpdateVisibility {
                template_id,
                visibility,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![VisibilityUpdated {
                    template_id,
                    visibility,
                    modified_at,
                }])
            }
            UpdateDescription {
                template_id,
                description,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![DescriptionUpdated {
                    template_id,
                    description,
                    modified_at,
                }])
            }
            UpdateType { template_id, r#type } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TypeUpdated {
                    template_id,
                    r#type,
                    modified_at,
                }])
            }
            UpdateSchema { template_id, schema } => {
                validate_json_schema(&schema)?;

                // Enforce immutable properties: reject if any property with immutable=true
                // is missing from the new schema.
                if let Some(ref existing_attrs) = self.schema_properties_attributes {
                    let new_property_keys = get_schema_property_keys(&schema);
                    let immutable_missing: Vec<&String> = existing_attrs
                        .iter()
                        .filter(|(_, attr)| attr.immutable)
                        .filter(|(k, _)| !new_property_keys.contains(*k))
                        .map(|(k, _)| k)
                        .collect();

                    if !immutable_missing.is_empty() {
                        let keys_str: Vec<&str> = immutable_missing.iter().map(|k| k.as_str()).collect();
                        return Err(TemplateError::NonRemovablePropertyViolation(format!(
                            "The following immutable properties cannot be removed from the schema: [{}]",
                            keys_str.join(", ")
                        )));
                    }
                }

                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                let mut events = vec![SchemaUpdated {
                    template_id: template_id.clone(),
                    schema: schema.clone(),
                    modified_at: modified_at.clone(),
                }];

                // Prune schema_properties_attributes whose keys no longer exist in the new schema.
                if let Some(ref existing_attrs) = self.schema_properties_attributes {
                    let new_property_keys = get_schema_property_keys(&schema);
                    let pruned: HashMap<String, PropertyAttribute> = existing_attrs
                        .iter()
                        .filter(|(k, _)| new_property_keys.contains(*k))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();

                    if pruned.len() != existing_attrs.len() {
                        events.push(SchemaPropertiesAttributesUpdated {
                            template_id,
                            schema_properties_attributes: pruned,
                            modified_at,
                        });
                    }
                }

                Ok(events)
            }
            UpdateSchemaPropertiesAttributes {
                template_id,
                schema_properties_attributes,
            } => {
                validate_schema_properties_attributes(&self.schema, &schema_properties_attributes)?;

                // The `immutable` field is system-determined by the data model and cannot
                // be altered through any command. Override it with the existing values.
                let schema_properties_attributes = enforce_immutable_flag(
                    schema_properties_attributes,
                    &self.schema_properties_attributes,
                );

                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![SchemaPropertiesAttributesUpdated {
                    template_id,
                    schema_properties_attributes,
                    modified_at,
                }])
            }
            DeleteTemplate { template_id } => Ok(vec![TemplateDeleted { template_id }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use TemplateEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            TemplateCreated {
                template_id,
                source_template_id,
                title,
                display,
                data_model,
                creator,
                holder_type,
                modified_at,
                tags,
                status,
                visibility,
                description,
                r#type,
                schema,
                schema_properties_attributes,
            } => {
                self.template_id = template_id;
                self.source_template_id = source_template_id;
                self.title = title;
                self.display = *display;
                self.data_model = data_model;
                self.creator = creator;
                self.holder_type = holder_type;
                self.modified_at.replace(modified_at);
                self.tags = tags;
                self.status = status;
                self.visibility = visibility;
                self.description = description;
                self.r#type = r#type;
                self.schema = schema;
                self.schema_properties_attributes = schema_properties_attributes;
            }
            TitleUpdated {
                template_id: _,
                title,
                modified_at,
            } => {
                self.title = Some(title);
                self.modified_at.replace(modified_at);
            }
            DisplayUpdated {
                template_id: _,
                display,
                modified_at,
            } => {
                self.display = Some(display);
                self.modified_at.replace(modified_at);
            }
            DataModelUpdated {
                template_id: _,
                data_model,
                modified_at,
            } => {
                self.data_model = Some(data_model);
                self.modified_at.replace(modified_at);
            }
            CreatorUpdated {
                template_id: _,
                creator,
                modified_at,
            } => {
                self.creator = Some(creator);
                self.modified_at.replace(modified_at);
            }
            HolderTypeUpdated {
                template_id: _,
                holder_type,
                modified_at,
            } => {
                self.holder_type = Some(holder_type);
                self.modified_at.replace(modified_at);
            }
            TagsUpdated {
                template_id: _,
                tags,
                modified_at,
            } => {
                self.tags = tags;
                self.modified_at.replace(modified_at);
            }
            StatusUpdated {
                template_id: _,
                status,
                modified_at,
            } => {
                self.status = status;
                self.modified_at.replace(modified_at);
            }
            VisibilityUpdated {
                template_id: _,
                visibility,
                modified_at,
            } => {
                self.visibility = visibility;
                self.modified_at.replace(modified_at);
            }
            DescriptionUpdated {
                template_id: _,
                description,
                modified_at,
            } => {
                self.description = Some(description);
                self.modified_at.replace(modified_at);
            }
            TypeUpdated {
                template_id: _,
                r#type,
                modified_at,
            } => {
                self.r#type = r#type;
                self.modified_at.replace(modified_at);
            }
            SchemaUpdated {
                template_id: _,
                schema,
                modified_at,
            } => {
                *self.schema = Some(schema);
                self.modified_at.replace(modified_at);
            }
            SchemaPropertiesAttributesUpdated {
                template_id: _,
                schema_properties_attributes,
                modified_at,
            } => {
                self.schema_properties_attributes = Some(schema_properties_attributes);
                self.modified_at.replace(modified_at);
            }
            TemplateDeleted { template_id } => {
                *self = Self::default();
                self.template_id = template_id;
                self.status = Status::Deleted;
            }
        }
    }
}

fn validate_json_schema(schema: &serde_json::Value) -> Result<(), TemplateError> {
    jsonschema::validator_for(schema)
        .map(|_| ())
        .map_err(|e| TemplateError::InvalidSchema(e.to_string()))
}

fn get_schema_property_keys(schema: &serde_json::Value) -> std::collections::HashSet<String> {
    schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default()
}

fn validate_schema_properties_attributes(
    schema: &Option<serde_json::Value>,
    attributes: &HashMap<String, PropertyAttribute>,
) -> Result<(), TemplateError> {
    let property_keys = match schema {
        Some(s) => get_schema_property_keys(s),
        None => std::collections::HashSet::new(),
    };

    let invalid_keys: Vec<&String> = attributes.keys().filter(|k| !property_keys.contains(*k)).collect();

    if !invalid_keys.is_empty() {
        let keys_str: Vec<&str> = invalid_keys.iter().map(|k| k.as_str()).collect();
        return Err(TemplateError::InvalidSchemaPropertiesAttributes(format!(
            "The following keys do not match any field in schema.properties: [{}]",
            keys_str.join(", ")
        )));
    }

    Ok(())
}

/// Ensures the `immutable` flag on each property attribute preserves the existing
/// system-determined value. Users cannot alter `immutable` through commands.
fn enforce_immutable_flag(
    mut new_attrs: HashMap<String, PropertyAttribute>,
    existing_attrs: &Option<HashMap<String, PropertyAttribute>>,
) -> HashMap<String, PropertyAttribute> {
    if let Some(existing) = existing_attrs {
        for (key, new_attr) in new_attrs.iter_mut() {
            if let Some(existing_attr) = existing.get(key) {
                new_attr.immutable = existing_attr.immutable;
            } else {
                // New properties not previously tracked default to non-immutable.
                new_attr.immutable = false;
            }
        }
    } else {
        // No existing attributes means no immutable flags to preserve.
        for attr in new_attrs.values_mut() {
            attr.immutable = false;
        }
    }
    new_attrs
}

#[cfg(test)]
pub mod document_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type TemplateTestFramework = TestFramework<Template>;

    #[allow(clippy::too_many_arguments)]
    #[rstest]
    #[serial_test::serial]
    async fn test_create_template(
        template_id: String,
        title: Option<String>,
        display: Option<Display>,
        data_model: Option<DataModel>,
        creator: Option<String>,
        holder_type: Option<HolderType>,
        modified_at: String,
        tags: Vec<String>,
        status: Status,
        visibility: Visibility,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Option<serde_json::Value>,
        schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    ) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id: template_id.clone(),
                source_template_id: None,
                title: title.clone(),
                display: Box::new(display.clone()),
                data_model: data_model.clone(),
                creator: creator.clone(),
                holder_type: holder_type.clone(),
                tags: tags.clone(),
                status: status.clone(),
                visibility: visibility.clone(),
                description: description.clone(),
                r#type: r#type.clone(),
                schema: Box::new(schema.clone()),
                schema_properties_attributes: schema_properties_attributes.clone(),
            })
            .then_expect_events(vec![TemplateEvent::TemplateCreated {
                template_id,
                source_template_id: None,
                title,
                display: Box::new(display),
                data_model,
                creator,
                holder_type,
                modified_at,
                tags,
                status,
                visibility,
                description,
                r#type,
                schema: Box::new(schema),
                schema_properties_attributes,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_with_invalid_schema(template_id: String) {
        let invalid_schema = serde_json::json!({
            "type": "not_a_valid_type"
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id,
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(invalid_schema)),
                schema_properties_attributes: None,
            })
            .then_expect_error_message("Invalid JSON Schema: \"not_a_valid_type\" is not valid under any of the schemas listed in the 'anyOf' keyword")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_with_no_schema(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            })
            .then_expect_events(vec![TemplateEvent::TemplateCreated {
                template_id,
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_schema_with_invalid_schema(template_id: String) {
        let invalid_schema = serde_json::json!({
            "type": "not_a_valid_type"
        });

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id,
                schema: invalid_schema,
            })
            .then_expect_error_message("Invalid JSON Schema: \"not_a_valid_type\" is not valid under any of the schemas listed in the 'anyOf' keyword")
    }

    #[rstest]
    #[serial_test::serial]
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
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: valid_schema.clone(),
            })
            .then_expect_events(vec![TemplateEvent::SchemaUpdated {
                template_id,
                schema: valid_schema,
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
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
                immutable: false,
            },
        );

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id,
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(schema)),
                schema_properties_attributes: Some(attrs),
            })
            .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [nonexistent]")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_with_attributes_but_no_schema(template_id: String) {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: false,
            },
        );

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id,
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: Some(attrs),
            })
            .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [name]")
    }

    #[rstest]
    #[serial_test::serial]
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
                immutable: false,
            },
        );

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(schema)),
                schema_properties_attributes: None,
            }])
            .when(TemplateCommand::UpdateSchemaPropertiesAttributes {
                template_id,
                schema_properties_attributes: attrs,
            })
            .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [nonexistent]")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_field_attributes_with_no_schema(template_id: String) {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: false,
            },
        );

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            }])
            .when(TemplateCommand::UpdateSchemaPropertiesAttributes {
                template_id,
                schema_properties_attributes: attrs,
            })
            .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [name]")
    }

    #[rstest]
    #[serial_test::serial]
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
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: false,
            },
        );
        attrs.insert(
            "age".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: false,
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
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: false,
            },
        );

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(original_schema)),
                schema_properties_attributes: Some(attrs),
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: new_schema.clone(),
            })
            .then_expect_events(vec![
                TemplateEvent::SchemaUpdated {
                    template_id: template_id.clone(),
                    schema: new_schema,
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
    async fn test_update_schema_no_prune_needed(template_id: String) {
        let original_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: false,
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
                title: None,
                display: Box::new(None),
                data_model: None,
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(original_schema)),
                schema_properties_attributes: Some(attrs),
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: new_schema.clone(),
            })
            .then_expect_events(vec![TemplateEvent::SchemaUpdated {
                template_id,
                schema: new_schema,
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_schema_rejects_removal_of_immutable_property(template_id: String) {
        let original_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" }
            }
        });

        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "age".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: false,
            },
        );

        // Try to remove the immutable "name" property
        let new_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "age": { "type": "integer" }
            }
        });

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: Some(DataModel::OpenBadges3_0),
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(original_schema)),
                schema_properties_attributes: Some(attrs),
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id,
                schema: new_schema,
            })
            .then_expect_error_message("Cannot remove immutable schema properties: The following immutable properties cannot be removed from the schema: [name]")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_schema_allows_removal_of_non_immutable_property(template_id: String) {
        let original_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" }
            }
        });

        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "age".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: false,
            },
        );

        // Remove non-immutable "age" property - should succeed
        let new_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let mut expected_attrs = HashMap::new();
        expected_attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: Some(DataModel::OpenBadges3_0),
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(original_schema)),
                schema_properties_attributes: Some(attrs),
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: new_schema.clone(),
            })
            .then_expect_events(vec![
                TemplateEvent::SchemaUpdated {
                    template_id: template_id.clone(),
                    schema: new_schema,
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
    async fn test_create_open_badges_template_auto_populates_immutable(template_id: String) {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievementName": { "type": "string" },
                "description": { "type": "string" }
            }
        });

        let mut expected_attrs = HashMap::new();
        expected_attrs.insert(
            "achievementName".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        expected_attrs.insert(
            "description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: Some(DataModel::OpenBadges3_0),
                creator: None,
                holder_type: None,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(schema.clone())),
                schema_properties_attributes: None,
            })
            .then_expect_events(vec![TemplateEvent::TemplateCreated {
                template_id,
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: Some(DataModel::OpenBadges3_0),
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(schema)),
                schema_properties_attributes: Some(expected_attrs),
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_attributes_cannot_change_immutable_flag(template_id: String) {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let mut existing_attrs = HashMap::new();
        existing_attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );

        // User tries to set immutable to false - it should be preserved as true
        let mut user_attrs = HashMap::new();
        user_attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: false, // User tries to change this
            },
        );

        let mut expected_attrs = HashMap::new();
        expected_attrs.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: true, // System preserves immutable
            },
        );

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: Some(DataModel::OpenBadges3_0),
                creator: None,
                holder_type: None,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(schema)),
                schema_properties_attributes: Some(existing_attrs),
            }])
            .when(TemplateCommand::UpdateSchemaPropertiesAttributes {
                template_id: template_id.clone(),
                schema_properties_attributes: user_attrs,
            })
            .then_expect_events(vec![TemplateEvent::SchemaPropertiesAttributesUpdated {
                template_id,
                schema_properties_attributes: expected_attrs,
                modified_at: test_utils::modified_at(),
            }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use rstest::fixture;

    #[fixture]
    pub fn template_id() -> String {
        "template_id".to_string()
    }

    #[fixture]
    pub fn title() -> Option<String> {
        Some("Sample Template".to_string())
    }

    #[fixture]
    pub fn display() -> Option<Display> {
        Some(Display {
            name: "Sample Display".to_string(),
            logo: None,
        })
    }

    #[fixture]
    pub fn data_model() -> Option<DataModel> {
        Some(DataModel::W3CVcDataModelV1_1)
    }

    #[fixture]
    pub fn creator() -> Option<String> {
        Some("Creator Name".to_string())
    }

    #[fixture]
    pub fn holder_type() -> Option<HolderType> {
        Some(HolderType::Individual)
    }

    #[fixture]
    pub fn modified_at() -> String {
        "2024-01-01T00:00:00Z".to_string()
    }

    #[fixture]
    pub fn tags() -> Vec<String> {
        vec!["tag1".to_string(), "tag2".to_string()]
    }

    #[fixture]
    pub fn status() -> Status {
        Status::Draft
    }

    #[fixture]
    pub fn visibility() -> Visibility {
        Visibility::Private
    }

    #[fixture]
    pub fn description() -> Option<String> {
        Some("Sample description".to_string())
    }

    #[fixture]
    pub fn r#type() -> Vec<String> {
        vec!["Type1".to_string(), "Type2".to_string()]
    }

    #[fixture]
    pub fn schema() -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        }))
    }

    #[fixture]
    pub fn schema_properties_attributes() -> Option<HashMap<String, PropertyAttribute>> {
        let mut config = HashMap::new();
        config.insert(
            "name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                immutable: false,
            },
        );
        Some(config)
    }
}
