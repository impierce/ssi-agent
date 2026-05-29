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
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Expiration {
    /// Never expires.
    Never,
    /// Relative duration in ISO 8601 format, e.g. `"P3DT4H"` or seconds as `"PT86400S"`.
    Duration(String),
    /// Absolute datetime in ISO 8601 format, e.g. `"2026-12-31T23:59:59Z"`.
    DateTime(String),
}

impl Default for Expiration {
    /// Defaults to a relative 90-day expiration (`P90D`).
    fn default() -> Self {
        Expiration::Duration("P90D".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PropertyAttribute {
    selectively_disclosable: bool,
    /// Whether this property is non-removable — its leaf field must remain present in the schema
    /// and cannot be removed by the caller. Determined by the data model; system-controlled only.
    /// For OpenBadges 3.0 templates, only the required standard fields are non-removable.
    /// Defaults to `false`.
    #[serde(default)]
    non_removable: bool,
}

impl PropertyAttribute {
    /// Creates a new `PropertyAttribute`.
    pub fn new(selectively_disclosable: bool, non_removable: bool) -> Self {
        Self {
            selectively_disclosable,
            non_removable,
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
    pub title: String,
    pub display: Option<Display>,
    pub data_model: DataModel,
    pub holder_type: HolderType,
    pub modified_at: Option<String>,
    pub tags: Option<Vec<String>>,
    pub status: Status,
    pub visibility: Visibility,
    pub credential_expiration: Expiration,
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
            CreateNewTemplate {
                template_id,
                source_template_id,
                title,
                display,
                data_model,
                holder_type,
                tags,
                status,
                visibility,
                credential_expiration,
                description,
                r#type,
                schema,
                schema_properties_attributes,
            } => {
                // Only Draft and Published are allowed on creation.
                if matches!(status, Status::Archived | Status::Deleted) {
                    return Err(TemplateError::InvalidStatusOnCreate);
                }

                // Validate that a title is provided
                let title = title.trim().to_string();
                if title.is_empty() {
                    return Err(TemplateError::MissingTitle);
                }

                // Normalize type (defaults, canonical order, dedup).
                let r#type = normalize_and_validate_type(r#type, &data_model)?;

                // Normalize tags.
                let tags = normalize_tags(tags);

                // Validate the credential expiration value if provided.
                let credential_expiration = credential_expiration.unwrap_or_default();
                validate_expiration(&credential_expiration)?;

                // W3C VC 1.1 does not expose claim metadata and must not have
                // schemaPropertiesAttributes.
                if data_model == DataModel::W3CVcDataModelV1_1
                    && schema_properties_attributes.as_ref().is_some_and(|a| !a.is_empty())
                {
                    return Err(TemplateError::SchemaPropertiesAttributesNotAllowed);
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
                    // Canonicalize schema (adds additionalProperties: false recursively).
                    canonicalize_schema(&mut s);
                    Box::new(Some(s))
                } else {
                    schema
                };

                if let Some(ref s) = *schema {
                    validate_json_schema(s)?;
                    validate_schema_no_array_types(s)?;

                    if data_model == DataModel::OpenBadges3_0 {
                        validate_open_badges_schema_properties(s)?;
                    }
                }

                // Canonicalize non-OB schemas.
                let schema = if data_model != DataModel::OpenBadges3_0 {
                    if let Some(mut s) = *schema {
                        canonicalize_schema(&mut s);
                        Box::new(Some(s))
                    } else {
                        Box::new(None)
                    }
                } else {
                    schema
                };

                if let Some(ref attrs) = schema_properties_attributes {
                    if !attrs.is_empty() {
                        validate_schema_properties_attributes(&schema, attrs)?;
                    }
                }

                // For OpenBadges 3.0 templates, auto-populate non_removable attributes
                // for schema properties. Only required standard fields are non-removable.
                let schema_properties_attributes = if data_model == DataModel::OpenBadges3_0 {
                    if let Some(ref s) = *schema {
                        let leaf_paths = collect_leaf_paths(s);
                        let required_paths = open_badges_required_leaf_paths();
                        let mut attrs = schema_properties_attributes.unwrap_or_default();
                        for key in leaf_paths {
                            let is_required = required_paths.contains(&key);
                            let attr = attrs.entry(key).or_insert(PropertyAttribute {
                                selectively_disclosable: false,
                                non_removable: false,
                            });
                            attr.non_removable = is_required;
                        }
                        Some(attrs)
                    } else {
                        schema_properties_attributes
                    }
                } else {
                    // Normalize empty map to None.
                    schema_properties_attributes.filter(|a| !a.is_empty())
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
                    holder_type,
                    modified_at,
                    tags,
                    status,
                    visibility,
                    credential_expiration,
                    description,
                    r#type,
                    schema,
                    schema_properties_attributes,
                }])
            }
            UpdateTitle { template_id, title } => {
                ensure_template_editable(&self.status)?;

                let title = title.trim().to_string();
                if title.is_empty() {
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
                ensure_template_editable(&self.status)?;

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
            UpdateTags { template_id, tags } => {
                ensure_template_editable(&self.status)?;

                // Normalize: trim, dedup, drop empty.  Empty result is stored as None via apply.
                let tags = normalize_tags(Some(tags)).unwrap_or_default();

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
                ensure_status_transition_allowed(&self.status, &status)?;

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
                ensure_template_editable(&self.status)?;

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
                ensure_template_editable(&self.status)?;

                // Trim whitespace.  An empty description clears the field (stored as None via apply).
                let description = description.trim().to_string();

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
                ensure_template_editable(&self.status)?;

                let r#type = normalize_and_validate_type(r#type, &self.data_model)?;

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
                ensure_template_editable(&self.status)?;

                validate_json_schema(&schema)?;
                validate_schema_no_array_types(&schema)?;

                if self.data_model == DataModel::OpenBadges3_0 {
                    validate_open_badges_schema_properties(&schema)?;
                    validate_open_badges_required_properties(&schema)?;
                }

                // Ensure required keys are included in schema.required for OpenBadges 3.0
                let mut schema = schema;
                if self.data_model == DataModel::OpenBadges3_0 {
                    ensure_schema_required_keys(&mut schema);
                }

                // Canonicalize schema: add `additionalProperties: false` recursively.
                canonicalize_schema(&mut schema);

                // Enforce non-removable properties: reject if any property with non_removable=true
                // is missing from the new schema.
                if let Some(ref existing_attrs) = self.schema_properties_attributes {
                    let new_leaf_paths = collect_leaf_paths(&schema);
                    let immutable_missing: Vec<&str> = existing_attrs
                        .iter()
                        .filter(|(_, attr)| attr.non_removable)
                        .filter(|(k, _)| !new_leaf_paths.contains(*k))
                        .map(|(k, _)| k.as_str())
                        .collect();

                    if !immutable_missing.is_empty() {
                        return Err(TemplateError::NonRemovablePropertyViolation(format!(
                            "The following non-removable properties cannot be removed from the schema: [{}]",
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
                    let new_leaf_paths = collect_leaf_paths(&schema);
                    let pruned: HashMap<String, PropertyAttribute> = existing_attrs
                        .iter()
                        .filter(|(k, _)| new_leaf_paths.contains(*k))
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
                ensure_template_editable(&self.status)?;

                // W3C VC 1.1 does not expose claim metadata and must not have
                // schemaPropertiesAttributes.
                if self.data_model == DataModel::W3CVcDataModelV1_1 {
                    return Err(TemplateError::SchemaPropertiesAttributesNotAllowed);
                }

                // Trim keys and detect collisions after trimming.
                let schema_properties_attributes = trim_and_deduplicate_attribute_keys(schema_properties_attributes)?;

                validate_schema_properties_attributes(&self.schema, &schema_properties_attributes)?;

                // The `non_removable` flag is system-determined by the data model and cannot
                // be altered through any command. Override it with the existing values.
                let schema_properties_attributes =
                    enforce_non_removable_flag(schema_properties_attributes, &self.schema_properties_attributes);

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
            DeleteTemplate { template_id } => {
                ensure_template_can_be_deleted(&self.status)?;
                Ok(vec![TemplateDeleted { template_id }])
            }
            UpdateCredentialExpiration {
                template_id,
                credential_expiration,
            } => {
                ensure_template_editable(&self.status)?;

                validate_expiration(&credential_expiration)?;

                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![CredentialExpirationUpdated {
                    template_id,
                    credential_expiration,
                    modified_at,
                }])
            }
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
                holder_type,
                modified_at,
                tags,
                status,
                visibility,
                credential_expiration,
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
                self.holder_type = holder_type;
                self.modified_at.replace(modified_at);
                self.tags = tags;
                self.status = status;
                self.visibility = visibility;
                self.credential_expiration = credential_expiration;
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
                self.title = title;
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
            TagsUpdated {
                template_id: _,
                tags,
                modified_at,
            } => {
                self.tags = if tags.is_empty() { None } else { Some(tags) };
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
                self.description = if description.is_empty() {
                    None
                } else {
                    Some(description)
                };
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
            CredentialExpirationUpdated {
                template_id: _,
                credential_expiration,
                modified_at,
            } => {
                self.credential_expiration = credential_expiration;
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

/// Validates that no field in the schema (at any nesting level) has `"type": "array"`.
///
/// Arrays are not supported in template schemas at this time. Supporting arrays would require
/// a strategy for addressing individual array item fields in `schemaPropertiesAttributes` and
/// in SD-JWT claim paths, which is deferred to a future release.
fn validate_schema_no_array_types(schema: &serde_json::Value) -> Result<(), TemplateError> {
    validate_no_array_types_recursive(schema)
}

fn validate_no_array_types_recursive(schema: &serde_json::Value) -> Result<(), TemplateError> {
    if let Some(type_val) = schema.get("type").and_then(|t| t.as_str()) {
        if type_val == "array" {
            return Err(TemplateError::InvalidSchema(
                "Array types are not supported in template schemas. \
                 Define only object and scalar fields."
                    .to_string(),
            ));
        }
    }

    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
        for value in properties.values() {
            validate_no_array_types_recursive(value)?;
        }
    }

    Ok(())
}

/// Normalizes and validates the `type` field.
///
/// - Drops blank/whitespace-only entries.
/// - For OpenBadges 3.0: enforces that exactly one badge-specific subtype is present,
///   that no extra types are included, and defaults to `['VerifiableCredential', 'OpenBadgeCredential']`
///   for empty or incomplete input.
/// - For all other data models: ensures `VerifiableCredential` is present (adds it if missing),
///   deduplicates, and applies canonical ordering (VerifiableCredential first).
fn normalize_and_validate_type(type_input: Vec<String>, data_model: &DataModel) -> Result<Vec<String>, TemplateError> {
    // Drop blank/whitespace-only entries.
    let filtered: Vec<String> = type_input.into_iter().filter(|t| !t.trim().is_empty()).collect();

    match data_model {
        DataModel::OpenBadges3_0 => normalize_open_badges_type(filtered),
        _ => normalize_standard_type(filtered),
    }
}

fn normalize_standard_type(filtered: Vec<String>) -> Result<Vec<String>, TemplateError> {
    const VC: &str = "VerifiableCredential";

    // Empty input defaults to ['VerifiableCredential'].
    if filtered.is_empty() {
        return Ok(vec![VC.to_string()]);
    }

    // Deduplicate preserving first-seen order.
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<String> = filtered.into_iter().filter(|t| seen.insert(t.clone())).collect();

    // Canonical order: VerifiableCredential first, then extras.
    let mut result = vec![VC.to_string()];
    for t in &deduped {
        if t != VC {
            result.push(t.clone());
        }
    }

    Ok(result)
}

fn normalize_open_badges_type(filtered: Vec<String>) -> Result<Vec<String>, TemplateError> {
    const OBC: &str = "OpenBadgeCredential";
    const AC: &str = "AchievementCredential";
    const VC: &str = "VerifiableCredential";

    // Empty or only-VC input defaults to ['VerifiableCredential', 'OpenBadgeCredential'].
    if filtered.is_empty() || (filtered.len() == 1 && filtered[0] == VC) {
        return Ok(vec![VC.to_string(), OBC.to_string()]);
    }

    let has_obc = filtered.contains(&OBC.to_string());
    let has_ac = filtered.contains(&AC.to_string());

    if has_obc && has_ac {
        return Err(TemplateError::InvalidType(
            "OpenBadges type cannot include both `OpenBadgeCredential` and `AchievementCredential`".to_string(),
        ));
    }

    // No extra types allowed beyond VC, OBC, AC.
    let allowed = [VC, OBC, AC];
    let extras: Vec<&str> = filtered
        .iter()
        .filter(|t| !allowed.contains(&t.as_str()))
        .map(|t| t.as_str())
        .collect();
    if !extras.is_empty() {
        return Err(TemplateError::InvalidType(format!(
            "OpenBadges type includes disallowed extra entries: [{}]",
            extras.join(", ")
        )));
    }

    // Must contain exactly one badge-specific subtype; deduplicate preserving order.
    let badge_type = if has_obc { OBC } else { AC };

    Ok(vec![VC.to_string(), badge_type.to_string()])
}

/// Normalizes a tags vector: trims each entry, drops empty entries, deduplicates
/// preserving first-seen order.  Returns `None` when the result would be empty.
fn normalize_tags(tags: Option<Vec<String>>) -> Option<Vec<String>> {
    let tags = tags?;

    let mut seen = std::collections::HashSet::new();
    let normalized: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .filter(|t| seen.insert(t.clone()))
        .collect();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Recursively adds `additionalProperties: false` to every JSON Schema object node
/// (a node that has `"type": "object"` or a `"properties"` key).
fn canonicalize_schema(schema: &mut serde_json::Value) {
    let obj = match schema.as_object_mut() {
        Some(obj) => obj,
        None => return,
    };

    let is_object_node = obj.get("type").and_then(|v| v.as_str()) == Some("object") || obj.contains_key("properties");

    if is_object_node {
        obj.entry("additionalProperties").or_insert(serde_json::json!(false));
    }

    // Collect property keys first to avoid borrowing issues.
    let property_keys: Vec<String> = obj
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();

    if property_keys.is_empty() {
        return;
    }

    // Re-borrow mutably to recurse into each property.
    if let Some(properties) = obj.get_mut("properties") {
        if let Some(properties_obj) = properties.as_object_mut() {
            for key in &property_keys {
                if let Some(prop) = properties_obj.get_mut(key) {
                    canonicalize_schema(prop);
                }
            }
        }
    }
}

fn ensure_template_editable(status: &Status) -> Result<(), TemplateError> {
    match status {
        Status::Draft | Status::Published => Ok(()),
        Status::Archived => Err(TemplateError::ArchivedTemplateImmutable),
        Status::Deleted => Err(TemplateError::DeletedTemplateTerminal),
    }
}

fn ensure_status_transition_allowed(current: &Status, next: &Status) -> Result<(), TemplateError> {
    use Status::*;

    match (current, next) {
        (Deleted, _) => Err(TemplateError::DeletedTemplateTerminal),
        (Draft, Draft | Published | Archived | Deleted) => Ok(()),
        (Published, Published | Archived) => Ok(()),
        (Published, Deleted) => Err(TemplateError::ArchiveBeforeDeleteRequired),
        (Archived, Archived | Published | Deleted) => Ok(()),
        (from, to) => Err(TemplateError::InvalidStatusTransition(format!(
            "cannot transition template status from `{}` to `{}`",
            status_name(from),
            status_name(to)
        ))),
    }
}

fn ensure_template_can_be_deleted(status: &Status) -> Result<(), TemplateError> {
    match status {
        Status::Draft | Status::Archived => Ok(()),
        Status::Published => Err(TemplateError::ArchiveBeforeDeleteRequired),
        Status::Deleted => Err(TemplateError::DeletedTemplateTerminal),
    }
}

fn status_name(status: &Status) -> &'static str {
    match status {
        Status::Draft => "draft",
        Status::Published => "published",
        Status::Archived => "archived",
        Status::Deleted => "deleted",
    }
}

fn validate_expiration(expiration: &Expiration) -> Result<(), TemplateError> {
    match expiration {
        Expiration::Never => Ok(()),
        Expiration::Duration(s) => iso8601::duration(s)
            .map(|_| ())
            .map_err(|_| TemplateError::InvalidExpiration(format!("`{s}` is not a valid ISO 8601 duration"))),
        Expiration::DateTime(s) => chrono::DateTime::parse_from_rfc3339(s)
            .map(|_| ())
            .map_err(|e| TemplateError::InvalidExpiration(format!("`{s}` is not a valid ISO 8601 datetime: {e}"))),
    }
}

/// Recursively collects JSON Pointer paths (RFC 6901) to all leaf fields in a JSON Schema.
///
/// Rules:
/// - Object nodes with defined, non-empty sub-properties are NOT added themselves; only their
///   leaf descendants are added.
/// - Object nodes with no defined sub-properties (or empty `properties`) are treated as leaves.
/// - Arrays are not currently supported and must be rejected separately.
/// - Paths follow RFC 6901 format: `/fieldName`, `/parent/child`, `/a/b/c`.
///
/// This matches how `schemaPropertiesAttributes` keys must be addressed, and maps 1:1 to
/// SD-JWT claim paths without any translation step.
fn collect_leaf_paths(schema: &serde_json::Value) -> std::collections::HashSet<String> {
    let mut paths = std::collections::HashSet::new();
    collect_leaf_paths_with_prefix(schema, "", &mut paths);
    paths
}

fn collect_leaf_paths_with_prefix(
    schema: &serde_json::Value,
    prefix: &str,
    paths: &mut std::collections::HashSet<String>,
) {
    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return,
    };

    for (key, value) in properties {
        // RFC 6901: escape `~` as `~0` and `/` as `~1` in key segments.
        let escaped_key = key.replace('~', "~0").replace('/', "~1");
        let path = format!("{}/{}", prefix, escaped_key);

        // A node is a "nested object" only when it has defined, non-empty sub-properties.
        // Such nodes are not added themselves — only their leaf descendants are.
        let has_sub_properties = value
            .get("properties")
            .and_then(|p| p.as_object())
            .is_some_and(|p| !p.is_empty());

        if has_sub_properties {
            collect_leaf_paths_with_prefix(value, &path, paths);
        } else {
            paths.insert(path);
        }
    }
}

fn validate_schema_properties_attributes(
    schema: &Option<serde_json::Value>,
    attributes: &HashMap<String, PropertyAttribute>,
) -> Result<(), TemplateError> {
    let leaf_paths = match schema {
        Some(s) => collect_leaf_paths(s),
        None => std::collections::HashSet::new(),
    };

    let mut invalid_keys: Vec<&str> = attributes
        .keys()
        .filter(|k| !leaf_paths.contains(*k))
        .map(|k| k.as_str())
        .collect();

    if !invalid_keys.is_empty() {
        invalid_keys.sort();
        return Err(TemplateError::InvalidSchemaPropertiesAttributes(format!(
            "The following keys do not match any field in schema.properties: [{}]",
            invalid_keys.join(", ")
        )));
    }

    Ok(())
}

/// Trims each key in `attributes` and returns a new map with trimmed keys.
/// Returns an error if two distinct keys collide after trimming.
fn trim_and_deduplicate_attribute_keys(
    attributes: HashMap<String, PropertyAttribute>,
) -> Result<HashMap<String, PropertyAttribute>, TemplateError> {
    let mut result: HashMap<String, PropertyAttribute> = HashMap::new();
    for (key, value) in attributes {
        let trimmed = key.trim().to_string();
        if result.contains_key(&trimmed) {
            return Err(TemplateError::DuplicateSchemaPropertiesAttributeKey(trimmed));
        }
        result.insert(trimmed, value);
    }
    Ok(result)
}

/// Returns the JSON Pointer paths (RFC 6901) of the leaf fields that are standard-mandated
/// required fields for OpenBadges 3.0 templates. These fields must always be present in the
/// template schema and their corresponding `PropertyAttribute` entries are `non_removable`.
pub fn open_badges_required_leaf_paths() -> Vec<String> {
    vec![
        "/achievement/name".to_string(),
        "/achievement/description".to_string(),
        "/achievement/criteria/narrative".to_string(),
    ]
}

/// Returns a lazily-initialised reference to the parsed OpenBadges 3.0 JSON Schema.
fn ob_json_schema() -> &'static serde_json::Value {
    static OB_SCHEMA: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    OB_SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../json_schemas/OpenBadgeCredentialV3.json"))
            .expect("OpenBadgeCredentialV3.json must be valid JSON")
    })
}

/// Validates that the nested OB template schema only uses property names that are valid
/// according to the OpenBadges 3.0 JSON Schema specification.
///
/// The full OB JSON Schema defines which properties are allowed at each nesting level via
/// its `$defs`. This function validates the template schema recursively against the relevant
/// `$defs` entries, so the allowlist is derived directly from the standard and does not need
/// to be maintained separately.
///
/// The known path-to-def mapping covers the most common nesting levels used in templates.
/// Paths not in the mapping are left open (any property name accepted at that level), which
/// means future OB sub-objects not yet added to the map will pass without error.
fn validate_open_badges_schema_properties(schema: &serde_json::Value) -> Result<(), TemplateError> {
    // Map from schema-traversal path to the OB $def name that governs that level.
    // The top-level template schema represents AchievementSubject content.
    let path_to_def: &[(&str, &str)] = &[
        ("", "AchievementSubject"),
        ("achievement", "Achievement"),
        ("achievement/criteria", "Criteria"),
        ("achievement/image", "Image"),
        ("achievement/creator", "Profile"),
        ("image", "Image"),
    ];

    let ob_schema = ob_json_schema();
    let defs = match ob_schema.get("$defs").and_then(|d| d.as_object()) {
        Some(d) => d,
        None => return Ok(()), // Cannot validate without defs; pass through.
    };

    validate_ob_properties_recursive(schema, "", path_to_def, defs)
}

fn validate_ob_properties_recursive(
    schema: &serde_json::Value,
    current_path: &str,
    path_to_def: &[(&str, &str)],
    defs: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), TemplateError> {
    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return Ok(()),
    };

    // Determine the set of allowed property names at this level.
    let allowed: Option<std::collections::HashSet<&str>> = path_to_def
        .iter()
        .find(|(p, _)| *p == current_path)
        .and_then(|(_, def_name)| defs.get(*def_name))
        .and_then(|def| def.get("properties").and_then(|p| p.as_object()))
        .map(|props| props.keys().map(|k| k.as_str()).collect());

    let mut disallowed: Vec<&str> = Vec::new();

    for (key, value) in properties {
        if let Some(ref allowed_set) = allowed {
            if !allowed_set.contains(key.as_str()) {
                disallowed.push(key.as_str());
            }
        }

        let child_path = if current_path.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", current_path, key)
        };

        validate_ob_properties_recursive(value, &child_path, path_to_def, defs)?;
    }

    if !disallowed.is_empty() {
        disallowed.sort();
        return Err(TemplateError::DisallowedOpenBadgesProperties(format!(
            "The following properties are not allowed for OpenBadges 3.0 templates at path `/{current_path}`: [{}]",
            disallowed.join(", ")
        )));
    }

    Ok(())
}

/// Validates that the three required OB leaf fields are present in the nested template schema
/// and that each has type `"string"` or a `"const"` value.
fn validate_open_badges_required_properties(schema: &serde_json::Value) -> Result<(), TemplateError> {
    // Helper: resolve a nested property by a slice of property key segments.
    fn resolve<'a>(schema: &'a serde_json::Value, segments: &[&str]) -> Option<&'a serde_json::Value> {
        let mut current = schema;
        for seg in segments {
            current = current.get("properties")?.get(*seg)?;
        }
        Some(current)
    }

    let required_fields: &[(&[&str], &str)] = &[
        (&["achievement", "name"], "achievement.name"),
        (&["achievement", "description"], "achievement.description"),
        (
            &["achievement", "criteria", "narrative"],
            "achievement.criteria.narrative",
        ),
    ];

    let mut missing: Vec<&str> = Vec::new();
    let mut wrong_type: Vec<&str> = Vec::new();

    for (segments, label) in required_fields {
        match resolve(schema, segments) {
            None => missing.push(label),
            Some(field) => {
                let has_type_string = field.get("type").and_then(|t| t.as_str()) == Some("string");
                let has_const = field.get("const").is_some();
                if !has_type_string && !has_const {
                    wrong_type.push(label);
                }
            }
        }
    }

    if !missing.is_empty() {
        return Err(TemplateError::MissingRequiredOpenBadgesProperties(format!(
            "The following required fields must be present in the schema for OpenBadges 3.0 templates: [{}]",
            missing.join(", ")
        )));
    }

    if !wrong_type.is_empty() {
        return Err(TemplateError::InvalidRequiredPropertyType(format!(
            "The following required fields must have type \"string\" or a \"const\" value: [{}]",
            wrong_type.join(", ")
        )));
    }

    Ok(())
}

/// Ensures that the OB-mandated `required` arrays are populated at the appropriate nesting
/// levels within the schema. Only adds entries that are not already present; does not remove
/// existing entries.
///
/// The following `required` entries are auto-managed:
/// - Root `required`: `["achievement"]`
/// - `achievement.required`: `["name", "description", "criteria"]`
/// - `achievement.criteria.required`: `["narrative"]`
fn ensure_schema_required_keys(schema: &mut serde_json::Value) {
    fn add_if_absent(arr: &mut serde_json::Value, key: &str) {
        if let Some(arr) = arr.as_array_mut() {
            let v = serde_json::Value::String(key.to_string());
            if !arr.contains(&v) {
                arr.push(v);
            }
        }
    }

    // Root level: ensure "achievement" is required.
    if schema.get("properties").and_then(|p| p.get("achievement")).is_some() {
        let root_required = schema
            .as_object_mut()
            .map(|o| o.entry("required").or_insert(serde_json::json!([])));
        if let Some(r) = root_required {
            add_if_absent(r, "achievement");
        }
    }

    // achievement level: ensure "name", "description", "criteria" are required.
    if let Some(achievement) = schema.get_mut("properties").and_then(|p| p.get_mut("achievement")) {
        let has_name = achievement.get("properties").and_then(|p| p.get("name")).is_some();
        let has_description = achievement
            .get("properties")
            .and_then(|p| p.get("description"))
            .is_some();
        let has_criteria = achievement.get("properties").and_then(|p| p.get("criteria")).is_some();

        if has_name || has_description || has_criteria {
            let ach_required = achievement
                .as_object_mut()
                .map(|o| o.entry("required").or_insert(serde_json::json!([])));
            if let Some(r) = ach_required {
                if has_name {
                    add_if_absent(r, "name");
                }
                if has_description {
                    add_if_absent(r, "description");
                }
                if has_criteria {
                    add_if_absent(r, "criteria");
                }
            }
        }
    }

    // achievement.criteria level: ensure "narrative" is required.
    if let Some(criteria) = schema
        .get_mut("properties")
        .and_then(|p| p.get_mut("achievement"))
        .and_then(|a| a.get_mut("properties"))
        .and_then(|p| p.get_mut("criteria"))
    {
        if criteria.get("properties").and_then(|p| p.get("narrative")).is_some() {
            let crit_required = criteria
                .as_object_mut()
                .map(|o| o.entry("required").or_insert(serde_json::json!([])));
            if let Some(r) = crit_required {
                add_if_absent(r, "narrative");
            }
        }
    }
}

/// Ensures the `non_removable` flag on each property attribute preserves the existing
/// system-determined value. Users cannot alter `non_removable` through commands.
fn enforce_non_removable_flag(
    mut new_attrs: HashMap<String, PropertyAttribute>,
    existing_attrs: &Option<HashMap<String, PropertyAttribute>>,
) -> HashMap<String, PropertyAttribute> {
    if let Some(existing) = existing_attrs {
        for (key, new_attr) in new_attrs.iter_mut() {
            if let Some(existing_attr) = existing.get(key) {
                new_attr.non_removable = existing_attr.non_removable;
            } else {
                // New properties not previously tracked default to non-removable=false.
                new_attr.non_removable = false;
            }
        }
    } else {
        // No existing attributes means no non-removable flags to preserve.
        for attr in new_attrs.values_mut() {
            attr.non_removable = false;
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[rstest]
    #[serial_test::serial]
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
            .when(TemplateCommand::CreateNewTemplate {
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
            })
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
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_without_title(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("A title is required when creating or updating a template")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_with_empty_title(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("A title is required when creating or updating a template")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_rejects_archived_status_on_create(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Invalid status on create: only Draft or Published are allowed")
    }

    #[rstest]
    #[serial_test::serial]
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
            }])
            .when(TemplateCommand::UpdateTitle {
                template_id,
                title: "   ".to_string(),
            })
            .then_expect_error_message("A title is required when creating or updating a template")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_title_on_archived_template_is_rejected(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(&template_id, Status::Archived)])
            .when(TemplateCommand::UpdateTitle {
                template_id,
                title: "Updated title".to_string(),
            })
            .then_expect_error_message("Archived templates are immutable except for status changes")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_status_from_published_to_draft_is_rejected(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(
                &template_id,
                Status::Published,
            )])
            .when(TemplateCommand::UpdateStatus {
                template_id,
                status: Status::Draft,
            })
            .then_expect_error_message(
                "Invalid status transition: cannot transition template status from `published` to `draft`",
            )
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_status_from_archived_to_published_is_allowed(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(&template_id, Status::Archived)])
            .when(TemplateCommand::UpdateStatus {
                template_id: template_id.clone(),
                status: Status::Published,
            })
            .then_expect_events(vec![TemplateEvent::StatusUpdated {
                template_id,
                status: Status::Published,
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_tags_normalizes_input(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
            .when(TemplateCommand::UpdateTags {
                template_id: template_id.clone(),
                tags: vec![
                    " alpha ".to_string(),
                    "".to_string(),
                    "beta".to_string(),
                    "alpha".to_string(),
                    "beta ".to_string(),
                ],
            })
            .then_expect_events(vec![TemplateEvent::TagsUpdated {
                template_id,
                tags: vec!["alpha".to_string(), "beta".to_string()],
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_description_trims_whitespace(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
            .when(TemplateCommand::UpdateDescription {
                template_id: template_id.clone(),
                description: "  trimmed description  ".to_string(),
            })
            .then_expect_events(vec![TemplateEvent::DescriptionUpdated {
                template_id,
                description: "trimmed description".to_string(),
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
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
            }])
            .when(TemplateCommand::UpdateType {
                template_id: template_id.clone(),
                r#type: vec![
                    "ExampleCredential".to_string(),
                    "VerifiableCredential".to_string(),
                    "ExampleCredential".to_string(),
                    "".to_string(),
                ],
            })
            .then_expect_events(vec![TemplateEvent::TypeUpdated {
                template_id,
                r#type: vec!["VerifiableCredential".to_string(), "ExampleCredential".to_string()],
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
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
            }])
            .when(TemplateCommand::UpdateType {
                template_id,
                r#type: vec![
                    "VerifiableCredential".to_string(),
                    "OpenBadgeCredential".to_string(),
                    "AchievementCredential".to_string(),
                ],
            })
            .then_expect_error_message(
                "Invalid type: OpenBadges type cannot include both `OpenBadgeCredential` and `AchievementCredential`",
            )
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_credential_expiration_emits_event(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
            .when(TemplateCommand::UpdateCredentialExpiration {
                template_id: template_id.clone(),
                credential_expiration: Expiration::Duration("P30D".to_string()),
            })
            .then_expect_events(vec![TemplateEvent::CredentialExpirationUpdated {
                template_id,
                credential_expiration: Expiration::Duration("P30D".to_string()),
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_update_credential_expiration_rejects_invalid_iso8601(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(&template_id, Status::Draft)])
            .when(TemplateCommand::UpdateCredentialExpiration {
                template_id,
                credential_expiration: Expiration::Duration("30 days".to_string()),
            })
            .then_expect_error_message("Invalid expiration value: `30 days` is not a valid ISO 8601 duration")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_open_badges_template_rejects_extra_types(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Invalid type: OpenBadges type includes disallowed extra entries: [ExtraType]")
    }

    #[rstest]
    #[serial_test::serial]
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
            }])
            .when(TemplateCommand::UpdateStatus {
                template_id,
                status: Status::Draft,
            })
            .then_expect_error_message(
                "Invalid status transition: cannot transition template status from `archived` to `draft`",
            )
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_open_badges_template_missing_required_achievement_properties(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message(
                "Missing required OpenBadges 3.0 schema properties: The following required fields must be present in the schema for OpenBadges 3.0 templates: [achievement.name, achievement.criteria.narrative]"
            )
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_open_badges_template_with_array_type_in_schema(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message(
                "Invalid JSON Schema: Array types are not supported in template schemas. Define only object and scalar fields."
            )
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_delete_template_from_published_requires_archive_first(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(
                &template_id,
                Status::Published,
            )])
            .when(TemplateCommand::DeleteTemplate { template_id })
            .then_expect_error_message("Published templates must be archived before they can be deleted")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_delete_template_from_archived_is_allowed(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![template_created_event_with_status(&template_id, Status::Archived)])
            .when(TemplateCommand::DeleteTemplate {
                template_id: template_id.clone(),
            })
            .then_expect_events(vec![TemplateEvent::TemplateDeleted { template_id }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_deleted_template_status_is_terminal(template_id: String) {
        TemplateTestFramework::with(())
            .given(vec![
                template_created_event_with_status(&template_id, Status::Draft),
                TemplateEvent::TemplateDeleted {
                    template_id: template_id.clone(),
                },
            ])
            .when(TemplateCommand::UpdateStatus {
                template_id,
                status: Status::Published,
            })
            .then_expect_error_message("Deleted templates are terminal and cannot be changed")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_with_invalid_schema(template_id: String) {
        let invalid_schema = serde_json::json!({
            "type": "not_a_valid_type"
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Invalid JSON Schema: \"not_a_valid_type\" is not valid under any of the schemas listed in the 'anyOf' keyword")
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_with_no_schema(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
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

        let canonical_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "additionalProperties": false
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
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: valid_schema,
            })
            .then_expect_events(vec![TemplateEvent::SchemaUpdated {
                template_id,
                schema: canonical_schema,
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
                non_removable: false,
            },
        );

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
                non_removable: false,
            },
        );

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [name]")
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("schemaPropertiesAttributes are not allowed for W3C VC 1.1 templates")
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
                non_removable: false,
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
                non_removable: false,
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
            }])
            .when(TemplateCommand::UpdateSchemaPropertiesAttributes {
                template_id,
                schema_properties_attributes: attrs,
            })
            .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [name]")
    }

    #[rstest]
    #[serial_test::serial]
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
            }])
            .when(TemplateCommand::UpdateSchemaPropertiesAttributes {
                template_id,
                schema_properties_attributes: attrs,
            })
            .then_expect_error_message("schemaPropertiesAttributes are not allowed for W3C VC 1.1 templates")
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );
        attrs.insert(
            " /name ".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: false,
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
            }])
            .when(TemplateCommand::UpdateSchemaPropertiesAttributes {
                template_id,
                schema_properties_attributes: attrs,
            })
            .then_expect_error_message("Duplicate schemaPropertiesAttributes key after trimming: `/name`")
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
            "/name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                non_removable: false,
            },
        );
        attrs.insert(
            "/age".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: false,
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
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: new_schema.clone(),
            })
            .then_expect_events(vec![
                TemplateEvent::SchemaUpdated {
                    template_id: template_id.clone(),
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "additionalProperties": false
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
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: new_schema,
            })
            .then_expect_events(vec![TemplateEvent::SchemaUpdated {
                template_id,
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "age": { "type": "integer" }
                    },
                    "additionalProperties": false
                }),
                modified_at: test_utils::modified_at(),
            }])
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );
        attrs.insert(
            "/description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: false,
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
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id,
                schema: new_schema,
            })
            .then_expect_error_message("Cannot remove immutable schema properties: The following non-removable properties cannot be removed from the schema: [/name]")
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );
        attrs.insert(
            "/achievement/description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );
        attrs.insert(
            "/achievement/criteria/narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );
        attrs.insert(
            "/achievement/tag".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: false,
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
            },
        );
        expected_attrs.insert(
            "/achievement/description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );
        expected_attrs.insert(
            "/achievement/criteria/narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );

        let expected_schema = serde_json::json!({
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
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id: template_id.clone(),
                schema: new_schema,
            })
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
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Missing required OpenBadges 3.0 schema properties: The following required fields must be present in the schema for OpenBadges 3.0 templates: [achievement.name, achievement.criteria.narrative]")
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );
        expected_attrs.insert(
            "/achievement/description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );
        expected_attrs.insert(
            "/achievement/criteria/narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );

        let expected_schema = serde_json::json!({
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

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
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
            }])
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );
        expected_attrs.insert(
            "/achievement/description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );
        expected_attrs.insert(
            "/achievement/criteria/narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );

        let expected_schema = serde_json::json!({
            "type": "object",
            "required": ["achievement"],
            "properties": {
                "achievement": {
                    "type": "object",
                    "required": ["name", "description", "criteria"],
                    "properties": {
                        "name": { "const": "Fixed Achievement Name" },
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

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
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
            "/name".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );

        // User tries to set non_removable to false — it should be preserved as true.
        let mut user_attrs = HashMap::new();
        user_attrs.insert(
            "/name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                non_removable: false, // User tries to change this
            },
        );

        let mut expected_attrs = HashMap::new();
        expected_attrs.insert(
            "/name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                non_removable: true, // System preserves non_removable
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

    #[rstest]
    #[serial_test::serial]
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
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Disallowed OpenBadges 3.0 schema properties: The following properties are not allowed for OpenBadges 3.0 templates at path `/`: [not_allowed_field]")
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );
        attrs.insert(
            "/achievement/description".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
            },
        );
        attrs.insert(
            "/achievement/criteria/narrative".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: true,
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
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id,
                schema: new_schema,
            })
            .then_expect_error_message("Disallowed OpenBadges 3.0 schema properties: The following properties are not allowed for OpenBadges 3.0 templates at path `/`: [invalid_field]")
    }

    #[rstest]
    #[serial_test::serial]
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
                },
            );
        }

        let expected_schema = serde_json::json!({
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
                                "narrative": { "type": "string" },
                                "id": { "type": "string" }
                            },
                            "additionalProperties": false
                        },
                        "image": { "type": "string" },
                        "achievementType": { "type": "string" },
                        "tag": { "type": "string" }
                    },
                    "additionalProperties": false
                }
            },
            "additionalProperties": false
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
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
            }])
    }

    // ── array-type rejection ─────────────────────────────────────────────────

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template_rejects_array_type(template_id: String) {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Invalid JSON Schema: Array types are not supported in template schemas. Define only object and scalar fields.")
    }

    #[rstest]
    #[serial_test::serial]
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
            }])
            .when(TemplateCommand::UpdateSchema {
                template_id,
                schema,
            })
            .then_expect_error_message("Invalid JSON Schema: Array types are not supported in template schemas. Define only object and scalar fields.")
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );
        attrs.insert(
            "/address/country".to_string(),
            PropertyAttribute {
                selectively_disclosable: false,
                non_removable: false,
            },
        );

        // The nested object `address` becomes an object node in the canonicalized schema.
        let expected_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" },
                        "country": { "type": "string" }
                    },
                    "additionalProperties": false
                }
            },
            "additionalProperties": false
        });

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
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
            }])
    }

    #[rstest]
    #[serial_test::serial]
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
            },
        );

        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateNewTemplate {
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
            })
            .then_expect_error_message("Invalid schema_properties_attributes key(s): The following keys do not match any field in schema.properties: [/address]")
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
    pub fn title() -> String {
        "Sample Template".to_string()
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
        DataModel::W3CVcDataModelV2_0
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
    pub fn tags() -> Option<Vec<String>> {
        Some(vec!["tag1".to_string(), "tag2".to_string()])
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
        vec![
            "VerifiableCredential".to_string(),
            "Type1".to_string(),
            "Type2".to_string(),
        ]
    }

    #[fixture]
    pub fn schema() -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "additionalProperties": false
        }))
    }

    #[fixture]
    pub fn schema_properties_attributes() -> Option<HashMap<String, PropertyAttribute>> {
        let mut config = HashMap::new();
        config.insert(
            "/name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                non_removable: false,
            },
        );
        Some(config)
    }
}
