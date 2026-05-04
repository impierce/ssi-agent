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

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq, utoipa::ToSchema)]
pub enum DataModel {
    // See https://www.w3.org/TR/vc-data-model-1.1/
    #[serde(rename = "w3c_vc_data_model_v1-1")]
    W3CVcDataModelV1_1,
    // See https://www.w3.org/TR/vc-data-model-2.0/
    #[serde(rename = "w3c_vc_data_model_v2-0")]
    #[default]
    W3CVcDataModelV2_0,
    // See https://www.imsglobal.org/spec/ob/v3p0/
    #[serde(rename = "open_badges_3-0")]
    OpenBadges3_0,
    // See https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model
    #[serde(rename = "european_learning_model_v3-3")]
    EuropeanLearningModelV3_3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HolderType {
    #[default]
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
    /// Whether this property is immutable (cannot be removed from the schema).
    /// Determined by the data model and cannot be altered through any command.
    /// For OpenBadges 3.0 templates, only the required properties
    /// (`achievement.name`, `achievement.description`, `achievement.criteria.narrative`) are immutable.
    /// Defaults to `false`.
    #[serde(default)]
    immutable: bool,
}

impl PropertyAttribute {
    /// Creates a new `PropertyAttribute`.
    pub fn new(selectively_disclosable: bool, immutable: bool) -> Self {
        Self {
            selectively_disclosable,
            immutable,
        }
    }

    /// Returns whether this property is selectively disclosable.
    pub fn is_selectively_disclosable(&self) -> bool {
        self.selectively_disclosable
    }
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
                // Validate that a title is provided
                match title {
                    None => return Err(TemplateError::MissingTitle),
                    Some(ref t) if t.trim().is_empty() => return Err(TemplateError::MissingTitle),
                    _ => {}
                }

                // For OpenBadges 3.0 templates, validate that the required properties are present
                // in the user-supplied schema. They are NOT auto-added.
                let schema = if data_model == DataModel::OpenBadges3_0 {
                    let mut s = (*schema).unwrap_or(serde_json::json!({"type": "object"}));
                    // Ensure schema has "type": "object" and "properties"
                    if let Some(obj) = s.as_object_mut() {
                        obj.entry("type").or_insert(serde_json::json!("object"));
                        obj.entry("properties").or_insert(serde_json::json!({}));
                    }
                    validate_open_badges_required_properties(&s)?;
                    // Ensure required keys are included in schema.required
                    ensure_schema_required_keys(&mut s);
                    Box::new(Some(s))
                } else {
                    schema
                };

                if let Some(ref s) = *schema {
                    validate_json_schema(s)?;

                    // For OpenBadges 3.0 templates, validate that all schema properties
                    // are within the allowed set.
                    if data_model == DataModel::OpenBadges3_0 {
                        validate_open_badges_schema_properties(s)?;
                    }
                }

                if let Some(ref attrs) = schema_properties_attributes {
                    validate_schema_properties_attributes(&schema, attrs)?;
                }

                // For OpenBadges 3.0 templates, auto-populate immutable attributes
                // for schema properties. Only required fields (achievement.name,
                // achievement.criteria.narrative) are immutable; optional fields are not.
                let schema_properties_attributes = if data_model == DataModel::OpenBadges3_0 {
                    if let Some(ref s) = *schema {
                        let property_keys = get_schema_property_keys(s);
                        let required_keys = open_badges_default_required_keys();
                        let mut attrs = schema_properties_attributes.unwrap_or_default();
                        for key in property_keys {
                            let is_required = required_keys.contains(&key);
                            let attr = attrs.entry(key).or_insert(PropertyAttribute {
                                selectively_disclosable: false,
                                immutable: false,
                            });
                            attr.immutable = is_required;
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
                if title.trim().is_empty() {
                    return Err(TemplateError::MissingTitle);
                }

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
                // data_model is immutable after creation
                if let Some(ref current) = self.data_model {
                    if *current != data_model {
                        let current_serialized = serde_json::to_value(current).unwrap_or_default();
                        let new_serialized = serde_json::to_value(&data_model).unwrap_or_default();
                        return Err(TemplateError::ImmutableDataModel(format!(
                            "The template uses the data_model `{}`, which is immutable after creation. If you wish to use the data_model `{}`, you must create a new template.",
                            current_serialized.as_str().unwrap_or("unknown"),
                            new_serialized.as_str().unwrap_or("unknown")
                        )));
                    }
                }

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
                // holder_type is immutable after creation
                if let Some(ref current) = self.holder_type {
                    if *current != holder_type {
                        let current_serialized = serde_json::to_value(current).unwrap_or_default();
                        let new_serialized = serde_json::to_value(&holder_type).unwrap_or_default();
                        return Err(TemplateError::ImmutableHolderType(format!(
                            "The template uses the holder_type `{}`, which is immutable after creation. If you wish to use the holder_type `{}`, you must create a new template.",
                            current_serialized.as_str().unwrap_or("unknown"),
                            new_serialized.as_str().unwrap_or("unknown")
                        )));
                    }
                }

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

                // For OpenBadges 3.0 templates, validate that all schema properties
                // are within the allowed set and required properties have correct type.
                if self.data_model == Some(DataModel::OpenBadges3_0) {
                    validate_open_badges_schema_properties(&schema)?;
                    validate_open_badges_required_properties(&schema)?;
                }

                // Ensure required keys are included in schema.required for OpenBadges 3.0
                let mut schema = schema;
                if self.data_model == Some(DataModel::OpenBadges3_0) {
                    ensure_schema_required_keys(&mut schema);
                }

                // Enforce immutable properties: reject if any property with immutable=true
                // is missing from the new schema.
                if let Some(ref existing_attrs) = self.schema_properties_attributes {
                    let new_property_keys = get_schema_property_keys(&schema);
                    let immutable_missing: Vec<&str> = existing_attrs
                        .iter()
                        .filter(|(_, attr)| attr.immutable)
                        .filter(|(k, _)| !new_property_keys.contains(*k))
                        .map(|(k, _)| k.as_str())
                        .collect();

                    if !immutable_missing.is_empty() {
                        return Err(TemplateError::NonRemovablePropertyViolation(format!(
                            "The following immutable properties cannot be removed from the schema: [{}]",
                            immutable_missing.join(", ")
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
                let schema_properties_attributes =
                    enforce_immutable_flag(schema_properties_attributes, &self.schema_properties_attributes);

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
                self.data_model = Some(data_model);
                self.creator = creator;
                self.holder_type = Some(holder_type);
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

/// Returns the default required schema properties for OpenBadges 3.0 templates.
/// These represent the standard-mandated fields that must always be present:
/// - `achievement.name`: The name of the achievement (user-provided)
/// - `achievement.description`: A description of the achievement (user-provided)
/// - `achievement.criteria.narrative`: Description of how the achievement is earned (user-provided)
///
/// The returned JSON value is a schema `properties` object suitable for merging into
/// a user-provided schema.
pub fn open_badges_default_schema_properties() -> serde_json::Value {
    serde_json::json!({
        "achievement.name": {
            "type": "string",
            "description": "The name of the achievement"
        },
        "achievement.description": {
            "type": "string",
            "description": "A description of the achievement"
        },
        "achievement.criteria.narrative": {
            "type": "string",
            "description": "Description of how the achievement is earned"
        }
    })
}

/// Returns the list of property keys that are required by the OpenBadges 3.0 standard.
pub fn open_badges_default_required_keys() -> Vec<String> {
    vec![
        "achievement.name".to_string(),
        "achievement.description".to_string(),
        "achievement.criteria.narrative".to_string(),
    ]
}

/// Returns the complete set of allowed schema property keys for OpenBadges 3.0 templates.
/// Only these keys may appear in the template's `schema.properties`.
///
/// This includes both required (immutable) and optional (removable) properties that conform
/// to the OpenBadges 3.0 standard for Achievement credentials.
pub fn open_badges_allowed_schema_property_keys() -> std::collections::HashSet<String> {
    [
        // Required/immutable fields
        "achievement.name",
        "achievement.description",
        "achievement.criteria.narrative",
        // Optional fields according to the OBv3 standard
        "achievement.criteria.id",
        "achievement.image",
        "achievement.achievementType",
        "achievement.tag",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Validates that all schema property keys for an OpenBadges 3.0 template are within
/// the allowed set of OBv3-conformant property keys.
fn validate_open_badges_schema_properties(schema: &serde_json::Value) -> Result<(), TemplateError> {
    let property_keys = get_schema_property_keys(schema);
    let allowed = open_badges_allowed_schema_property_keys();

    let disallowed: Vec<&String> = property_keys.iter().filter(|k| !allowed.contains(*k)).collect();

    if !disallowed.is_empty() {
        let mut sorted: Vec<&str> = disallowed.iter().map(|k| k.as_str()).collect();
        sorted.sort();
        return Err(TemplateError::DisallowedOpenBadgesProperties(format!(
            "The following properties are not allowed for OpenBadges 3.0 templates: [{}]. Allowed properties: [{}]",
            sorted.join(", "),
            {
                let mut allowed_sorted: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
                allowed_sorted.sort();
                allowed_sorted.join(", ")
            }
        )));
    }

    Ok(())
}

/// Validates that the required OpenBadges 3.0 properties are present in the schema
/// and that their type is fixed to "string".
/// Returns an error if any required property is missing or has an incorrect type.
fn validate_open_badges_required_properties(schema: &serde_json::Value) -> Result<(), TemplateError> {
    let property_keys = get_schema_property_keys(schema);
    let required_keys = open_badges_default_required_keys();

    let missing: Vec<&str> = required_keys
        .iter()
        .filter(|k| !property_keys.contains(*k))
        .map(|k| k.as_str())
        .collect();

    if !missing.is_empty() {
        return Err(TemplateError::MissingRequiredOpenBadgesProperties(format!(
            "The following required properties must be included in the schema for OpenBadges 3.0 templates: [{}]",
            missing.join(", ")
        )));
    }

    // Enforce that required properties have type "string" or a "const" value
    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
        let wrong_type: Vec<&str> = required_keys
            .iter()
            .filter(|k| {
                let prop = properties.get(k.as_str());
                let has_type_string = prop.and_then(|p| p.get("type")).and_then(|t| t.as_str()) == Some("string");
                let has_const = prop.and_then(|p| p.get("const")).is_some();
                !has_type_string && !has_const
            })
            .map(|k| k.as_str())
            .collect();

        if !wrong_type.is_empty() {
            return Err(TemplateError::InvalidRequiredPropertyType(format!(
                "The following required properties must have type \"string\" or a \"const\" value: [{}]",
                wrong_type.join(", ")
            )));
        }
    }

    Ok(())
}

/// Ensures that the OpenBadges 3.0 required property keys are included in the schema's
/// `required` array. Only adds keys that are present in `schema.properties`.
fn ensure_schema_required_keys(schema: &mut serde_json::Value) {
    let required_keys = open_badges_default_required_keys();
    let property_keys = get_schema_property_keys(schema);

    let schema_obj = match schema.as_object_mut() {
        Some(obj) => obj,
        None => return,
    };

    let required = schema_obj.entry("required").or_insert(serde_json::json!([]));
    if let Some(required_arr) = required.as_array_mut() {
        for key in &required_keys {
            if property_keys.contains(key) {
                let key_val = serde_json::Value::String(key.clone());
                if !required_arr.contains(&key_val) {
                    required_arr.push(key_val);
                }
            }
        }
    }
}

/// Merges OpenBadges 3.0 default required properties into a user-provided schema.
/// Ensures that the standard-mandated fields are always present and required.
///
/// # Panics
/// Panics if `schema` is not a JSON object. Callers must ensure the schema is an object
/// (or default to `{"type": "object"}`) before calling this function.
#[cfg(test)]
fn merge_open_badges_defaults(schema: &mut serde_json::Value) {
    let default_props = open_badges_default_schema_properties();
    let default_required = open_badges_default_required_keys();

    // Ensure schema has "type": "object"
    let schema_obj = schema
        .as_object_mut()
        .expect("merge_open_badges_defaults: schema must be a JSON object");
    schema_obj.entry("type").or_insert(serde_json::json!("object"));

    // Merge default properties into schema.properties
    let properties = schema_obj.entry("properties").or_insert(serde_json::json!({}));

    if let (Some(props_obj), Some(default_obj)) = (properties.as_object_mut(), default_props.as_object()) {
        for (key, value) in default_obj {
            props_obj.entry(key.clone()).or_insert(value.clone());
        }
    }

    // Merge default required keys into schema.required
    let required = schema
        .as_object_mut()
        .expect("merge_open_badges_defaults: schema must be a JSON object")
        .entry("required")
        .or_insert(serde_json::json!([]));

    if let Some(required_arr) = required.as_array_mut() {
        for key in &default_required {
            let key_val = serde_json::Value::String(key.clone());
            if !required_arr.contains(&key_val) {
                required_arr.push(key_val);
            }
        }
    }
}

/// Maps a flat credential input (conforming to an OpenBadges 3.0 template schema)
/// to the nested OBv3 credential structure expected by the issuance pipeline.
///
/// Dot-notation keys in the flat input are expanded into nested objects. For example:
/// `{"achievement.name": "Teamwork", "achievement.criteria.narrative": "..."}` becomes:
/// `{"credentialSubject": {"achievement": {"name": "Teamwork", "type": "Achievement", "criteria": {"narrative": "..."}}}}`
///
/// Additionally, the fixed value `achievement.type = "Achievement"` is injected.
pub fn map_open_badges_input_to_credential(flat_input: &serde_json::Value) -> serde_json::Value {
    let mut achievement = serde_json::Map::new();
    let mut criteria = serde_json::Map::new();
    let mut other_fields = serde_json::Map::new();

    if let Some(obj) = flat_input.as_object() {
        for (key, value) in obj {
            if let Some(suffix) = key.strip_prefix("achievement.criteria.") {
                criteria.insert(suffix.to_string(), value.clone());
            } else if let Some(suffix) = key.strip_prefix("achievement.") {
                achievement.insert(suffix.to_string(), value.clone());
            } else {
                other_fields.insert(key.clone(), value.clone());
            }
        }
    }

    // Insert criteria into achievement if any criteria fields exist
    if !criteria.is_empty() {
        achievement.insert("criteria".to_string(), serde_json::Value::Object(criteria));
    }

    // Inject fixed value: achievement.type = "Achievement"
    achievement
        .entry("type".to_string())
        .or_insert(serde_json::json!("Achievement"));

    // Inject achievement.id if not already provided
    achievement
        .entry("id".to_string())
        .or_insert_with(|| serde_json::json!(format!("urn:uuid:{}", uuid::Uuid::new_v4())));

    // Build the credentialSubject
    let mut credential_subject = serde_json::Map::new();
    credential_subject.insert("type".to_string(), serde_json::json!(["AchievementSubject"]));
    credential_subject.insert("achievement".to_string(), serde_json::Value::Object(achievement));

    // Build the final credential object
    let mut credential = serde_json::Map::new();
    credential.insert(
        "credentialSubject".to_string(),
        serde_json::Value::Object(credential_subject),
    );

    // Include any other non-achievement fields at the top level
    for (key, value) in other_fields {
        credential.insert(key, value);
    }

    serde_json::Value::Object(credential)
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
        data_model: DataModel,
        creator: Option<String>,
        holder_type: HolderType,
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
    async fn test_create_template_without_title(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id,
                source_template_id: None,
                title: None,
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            })
            .then_expect_error_message("A title is required when creating or updating a template")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_with_empty_title(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id,
                source_template_id: None,
                title: Some("".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            })
            .then_expect_error_message("A title is required when creating or updating a template")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_title_with_empty_string(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: Some("Original".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
            }])
            .when(TemplateCommand::UpdateTitle {
                template_id,
                title: "   ".to_string(),
            })
            .then_expect_error_message("A title is required when creating or updating a template")
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                creator: None,
                holder_type: HolderType::Individual,
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
                "achievement.name": { "type": "string" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" }
            }
        });

        let mut attrs = HashMap::new();
        attrs.insert(
            "achievement.name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "achievement.description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "achievement.criteria.narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );

        // Try to remove the immutable "achievement.name" property
        let new_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" }
            }
        });

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
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
            .then_expect_error_message("Missing required OpenBadges 3.0 schema properties: The following required properties must be included in the schema for OpenBadges 3.0 templates: [achievement.name]")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_schema_allows_removal_of_non_immutable_property(template_id: String) {
        let original_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" },
                "achievement.tag": { "type": "string" }
            }
        });

        let mut attrs = HashMap::new();
        attrs.insert(
            "achievement.name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "achievement.description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "achievement.criteria.narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "achievement.tag".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: false,
            },
        );

        // Remove non-immutable "achievement.tag" property - should succeed
        let new_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" }
            }
        });

        let mut expected_attrs = HashMap::new();
        expected_attrs.insert(
            "achievement.name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        expected_attrs.insert(
            "achievement.description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        expected_attrs.insert(
            "achievement.criteria.narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
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
                    schema: {
                        let mut expected_schema = new_schema;
                        expected_schema.as_object_mut().unwrap().insert(
                            "required".to_string(),
                            serde_json::json!([
                                "achievement.name",
                                "achievement.description",
                                "achievement.criteria.narrative"
                            ]),
                        );
                        expected_schema
                    },
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
    async fn test_create_open_badges_template_errors_when_required_properties_missing(template_id: String) {
        // Schema only has optional fields, missing required ones
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.description": { "type": "string" },
                "achievement.tag": { "type": "string" }
            }
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id,
                source_template_id: None,
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(schema)),
                schema_properties_attributes: None,
            })
            .then_expect_error_message("Missing required OpenBadges 3.0 schema properties: The following required properties must be included in the schema for OpenBadges 3.0 templates: [achievement.name, achievement.criteria.narrative]")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_open_badges_template_succeeds_with_required_properties(template_id: String) {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" },
                "achievement.description": { "type": "string" }
            }
        });

        let mut expected_attrs = HashMap::new();
        expected_attrs.insert(
            "achievement.name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        expected_attrs.insert(
            "achievement.criteria.narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        expected_attrs.insert(
            "achievement.description".to_string(),
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some({
                    let mut expected_schema = schema;
                    expected_schema.as_object_mut().unwrap().insert(
                        "required".to_string(),
                        serde_json::json!([
                            "achievement.name",
                            "achievement.description",
                            "achievement.criteria.narrative"
                        ]),
                    );
                    expected_schema
                })),
                schema_properties_attributes: Some(expected_attrs),
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_open_badges_template_succeeds_with_const_required_properties(template_id: String) {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "const": "Fixed Achievement Name" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" }
            }
        });

        let mut expected_attrs = HashMap::new();
        expected_attrs.insert(
            "achievement.name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        expected_attrs.insert(
            "achievement.description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        expected_attrs.insert(
            "achievement.criteria.narrative".to_string(),
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some({
                    let mut expected_schema = schema;
                    expected_schema.as_object_mut().unwrap().insert(
                        "required".to_string(),
                        serde_json::json!([
                            "achievement.name",
                            "achievement.description",
                            "achievement.criteria.narrative"
                        ]),
                    );
                    expected_schema
                })),
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
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

    #[test]
    fn test_map_open_badges_input_to_credential() {
        let flat_input = serde_json::json!({
            "achievement.name": "Teamwork",
            "achievement.criteria.narrative": "Team members are nominated for this badge by their peers."
        });

        let result = map_open_badges_input_to_credential(&flat_input);

        assert_eq!(result["credentialSubject"]["achievement"]["name"], "Teamwork");
        assert_eq!(result["credentialSubject"]["achievement"]["type"], "Achievement");
        assert_eq!(
            result["credentialSubject"]["achievement"]["criteria"]["narrative"],
            "Team members are nominated for this badge by their peers."
        );
        assert_eq!(
            result["credentialSubject"]["type"],
            serde_json::json!(["AchievementSubject"])
        );
        // achievement.id is auto-generated as a urn:uuid
        let id = result["credentialSubject"]["achievement"]["id"]
            .as_str()
            .expect("achievement.id should be a string");
        assert!(
            id.starts_with("urn:uuid:"),
            "achievement.id should start with 'urn:uuid:', got: {id}"
        );
    }

    #[test]
    fn test_map_open_badges_input_with_extra_fields() {
        let flat_input = serde_json::json!({
            "achievement.name": "Teamwork",
            "achievement.criteria.narrative": "Nominated by peers.",
            "achievement.description": "Collaboration badge",
            "id": "https://example.com/credentials/3527"
        });

        let result = map_open_badges_input_to_credential(&flat_input);

        assert_eq!(result["credentialSubject"]["achievement"]["name"], "Teamwork");
        assert_eq!(result["credentialSubject"]["achievement"]["type"], "Achievement");
        assert_eq!(
            result["credentialSubject"]["achievement"]["criteria"]["narrative"],
            "Nominated by peers."
        );
        assert_eq!(
            result["credentialSubject"]["achievement"]["description"],
            "Collaboration badge"
        );
        assert_eq!(result["id"], "https://example.com/credentials/3527");
        // achievement.id is auto-generated as a urn:uuid
        let id = result["credentialSubject"]["achievement"]["id"]
            .as_str()
            .expect("achievement.id should be a string");
        assert!(
            id.starts_with("urn:uuid:"),
            "achievement.id should start with 'urn:uuid:', got: {id}"
        );
    }

    #[test]
    fn test_open_badges_default_schema_properties() {
        let props = open_badges_default_schema_properties();
        assert!(props.get("achievement.name").is_some());
        assert!(props.get("achievement.criteria.narrative").is_some());
    }

    #[test]
    fn test_merge_open_badges_defaults_into_empty_schema() {
        let mut schema = serde_json::json!({});
        merge_open_badges_defaults(&mut schema);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("achievement.name"));
        assert!(props.contains_key("achievement.criteria.narrative"));

        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&serde_json::json!("achievement.name")));
        assert!(required.contains(&serde_json::json!("achievement.criteria.narrative")));
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_open_badges_template_rejects_disallowed_properties(template_id: String) {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" },
                "not_allowed_field": { "type": "string" }
            }
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id,
                source_template_id: None,
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some(schema)),
                schema_properties_attributes: None,
            })
            .then_expect_error_message("Disallowed OpenBadges 3.0 schema properties: The following properties are not allowed for OpenBadges 3.0 templates: [not_allowed_field]. Allowed properties: [achievement.achievementType, achievement.criteria.id, achievement.criteria.narrative, achievement.description, achievement.image, achievement.name, achievement.tag]")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_schema_rejects_disallowed_open_badges_properties(template_id: String) {
        let original_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" }
            }
        });

        let mut attrs = HashMap::new();
        attrs.insert(
            "achievement.name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "achievement.description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );
        attrs.insert(
            "achievement.criteria.narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                immutable: true,
            },
        );

        let new_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" },
                "invalid_field": { "type": "string" }
            }
        });

        TemplateTestFramework::with(())
            .given(vec![TemplateEvent::TemplateCreated {
                template_id: template_id.clone(),
                source_template_id: None,
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
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
            .then_expect_error_message("Disallowed OpenBadges 3.0 schema properties: The following properties are not allowed for OpenBadges 3.0 templates: [invalid_field]. Allowed properties: [achievement.achievementType, achievement.criteria.id, achievement.criteria.narrative, achievement.description, achievement.image, achievement.name, achievement.tag]")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_open_badges_template_allows_valid_optional_properties(template_id: String) {
        // All allowed optional properties plus the required ones should pass validation
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" },
                "achievement.criteria.narrative": { "type": "string" },
                "achievement.description": { "type": "string" },
                "achievement.criteria.id": { "type": "string" },
                "achievement.image": { "type": "string" },
                "achievement.achievementType": { "type": "string" },
                "achievement.tag": { "type": "string" }
            }
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id: template_id.clone(),
                source_template_id: None,
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
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
                title: Some("Test".to_string()),
                display: Box::new(None),
                data_model: DataModel::OpenBadges3_0,
                creator: None,
                holder_type: HolderType::Individual,
                modified_at: test_utils::modified_at(),
                tags: vec![],
                status: Status::Draft,
                visibility: Visibility::Private,
                description: None,
                r#type: vec![],
                schema: Box::new(Some({
                    let mut expected_schema = schema;
                    expected_schema.as_object_mut().unwrap().insert(
                        "required".to_string(),
                        serde_json::json!([
                            "achievement.name",
                            "achievement.description",
                            "achievement.criteria.narrative"
                        ]),
                    );
                    expected_schema
                })),
                schema_properties_attributes: Some({
                    let mut attrs = HashMap::new();
                    let required_keys = open_badges_default_required_keys();
                    for key in [
                        "achievement.name",
                        "achievement.criteria.narrative",
                        "achievement.description",
                        "achievement.criteria.id",
                        "achievement.image",
                        "achievement.achievementType",
                        "achievement.tag",
                    ] {
                        attrs.insert(
                            key.to_string(),
                            PropertyAttribute {
                                selectively_disclosable: false,
                                immutable: required_keys.contains(&key.to_string()),
                            },
                        );
                    }
                    attrs
                }),
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
    pub fn data_model() -> DataModel {
        DataModel::W3CVcDataModelV1_1
    }

    #[fixture]
    pub fn creator() -> Option<String> {
        Some("Creator Name".to_string())
    }

    #[fixture]
    pub fn holder_type() -> HolderType {
        HolderType::Individual
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
