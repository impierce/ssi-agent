use std::collections::HashMap;

use agent_shared::config::Authorization;
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tracing::{debug, info};

pub use super::open_badges::open_badges_required_leaf_paths;
use super::open_badges::{
    ensure_schema_required_keys, validate_open_badges_required_properties, validate_open_badges_schema_properties,
};
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
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
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
    pub selectively_disclosable: bool,
    /// Whether this property is non-removable — its leaf field must remain present in the schema
    /// and cannot be removed by the caller. Determined by the data model; system-controlled only.
    /// For OpenBadges 3.0 templates, only the required standard fields are non-removable.
    /// Defaults to `false`.
    #[serde(default)]
    pub non_removable: bool,
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

    /// Returns a copy of this attribute with `non_removable` forced to `false`.
    /// Use at API boundaries to ensure callers cannot set system-controlled flags.
    pub fn strip_non_removable(self) -> Self {
        Self {
            non_removable: false,
            ..self
        }
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
    /// JSON schema which defines the structure of the actual content of the credential.
    pub schema: Box<Option<serde_json::Value>>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    pub holder_authorization: Authorization,
}

impl Aggregate for Template {
    type Command = TemplateCommand;
    type Event = TemplateEvent;
    type Error = TemplateError;
    type Services = ();

    const TYPE: &'static str = "template";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use TemplateCommand::*;
        use TemplateEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
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
                holder_authorization,
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
                    holder_authorization,
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

                if self.status == Status::Draft && visibility == Visibility::Public {
                    return Err(TemplateError::DraftTemplateCannotBePublic);
                }
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
            UpdateHolderAuthorization {
                template_id,
                holder_authorization,
            } => {
                ensure_template_editable(&self.status)?;

                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![HolderAuthorizationUpdated {
                    template_id,
                    holder_authorization,
                    modified_at,
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
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
                holder_authorization,
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
                self.holder_authorization = holder_authorization;
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
            HolderAuthorizationUpdated {
                template_id: _,
                holder_authorization,
                modified_at,
            } => {
                self.holder_authorization = holder_authorization;
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
#[path = "aggregate_tests.rs"]
mod document_tests;

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
            }
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
