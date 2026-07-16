use crate::server_config::command::ServerConfigCommand;
use crate::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_library::template::aggregate::{DataModel, HolderType, PropertyAttribute, Template};
use agent_library::template::views::TemplateView;
use agent_shared::config::CredentialConfiguration;
use agent_shared::handlers::{command_handler, public_query_handler};
use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};
use oid4vc_core::claim_path_pointer::{ClaimPathElement, ClaimPathPointer};
use oid4vci::credential_issuer::credential_configurations_supported::{
    ClaimDescription, CredentialConfigurationsSupportedDisplay, CredentialMetadata, Logo as OidcLogo,
};
use shared_kernel::view_repository::DynViewRepository;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use tracing::warn;

type TemplateViewHandle = Arc<OnceLock<Arc<dyn DynViewRepository<TemplateView, Template>>>>;

pub struct CredentialConfigurationProjection {
    issuance_state: Arc<IssuanceState>,
    /// The template view repository used to re-query the current template state on partial updates.
    ///
    /// This is a `OnceLock` so that the real library state's view repository can be injected AFTER
    /// the library state (and hence the CQRS framework) is constructed, avoiding the circular
    /// dependency: projection → library state → projection. By the time any template events arrive the
    /// application is already started and the lock is set.
    template_view: TemplateViewHandle,
}

impl CredentialConfigurationProjection {
    /// Creates a new projection.
    ///
    /// Returns both the projection and the `OnceLock` handle.  After building the library state that
    /// owns this projection, call `handle.set(library_state.query.template.clone()).unwrap()` to wire
    /// the real (shared) view repository into the projection.
    pub fn new(issuance_state: Arc<IssuanceState>) -> (Self, TemplateViewHandle) {
        let template_view = Arc::new(OnceLock::new());
        let projection = Self {
            issuance_state,
            template_view: template_view.clone(),
        };
        (projection, template_view)
    }

    /// Re-queries the current template state from the view repository and keeps the corresponding
    /// credential configuration synchronized for published templates only.
    async fn sync_from_view(&self, template_id: &str) {
        use agent_library::template::aggregate::Status;

        let Some(view) = self.template_view.get() else {
            warn!("Template view not yet initialized; skipping credential configuration sync for `{template_id}`");
            return;
        };
        match public_query_handler(template_id, view).await {
            Ok(Some(template)) => {
                if template.status != Status::Published {
                    self.remove_credential_configuration(template_id).await;
                    return;
                }
                let credential_configuration = credential_configuration_from_template(&template);
                let command = ServerConfigCommand::UpdateCredentialConfiguration {
                    credential_configuration,
                    provisioned: false,
                };
                if let Err(e) = command_handler(
                    self.issuance_state.authorization_checker.clone(),
                    None,
                    SERVER_CONFIG_ID,
                    &self.issuance_state.command.server_config,
                    command,
                )
                .await
                {
                    warn!("Failed to update credential configuration for template `{template_id}`: {e}");
                }
            }
            Ok(None) => {
                warn!("Template `{template_id}` not found when trying to sync credential configuration");
            }
            Err(e) => {
                warn!("Failed to query template `{template_id}` for credential configuration sync: {e}");
            }
        }
    }

    /// Removes the credential configuration associated with the given template ID.
    async fn remove_credential_configuration(&self, template_id: &str) {
        let command = ServerConfigCommand::RemoveCredentialConfiguration {
            credential_configuration_id: template_id.to_string(),
            provisioned: false,
        };
        if let Err(e) = command_handler(
            self.issuance_state.authorization_checker.clone(),
            None,
            SERVER_CONFIG_ID,
            &self.issuance_state.command.server_config,
            command,
        )
        .await
        {
            warn!("Failed to remove credential configuration for template `{template_id}`: {e}");
        }
    }
}

/// Derives a `CredentialConfiguration` from a `Template`.
///
/// The display name is taken from `template.display.name` if present, falling back to `template.title`.
/// When the format is "vc+sd-jwt", claims are derived from `schema.properties` merged with
/// `schema_properties_attributes.selectivelyDisclosable`.
///
/// The `credential_definition.type` array prefers the template's explicit `type` values.
/// Narrow fallbacks are only used when the template does not yet provide any type values.
fn credential_configuration_from_template(template: &Template) -> CredentialConfiguration {
    let format = match template.data_model {
        DataModel::W3CVcDataModelV1_1 => "jwt_vc_json",
        _ if template.holder_type == HolderType::Organization => "jwt_vc_json",
        _ => "vc+sd-jwt",
    }
    .to_string();

    let type_ = if template.r#type.is_empty() {
        match template.data_model {
            DataModel::W3CVcDataModelV1_1 | DataModel::W3CVcDataModelV2_0 => {
                vec!["VerifiableCredential".to_string()]
            }
            DataModel::OpenBadges3_0 => {
                vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()]
            }
            _ => template.r#type.clone(),
        }
    } else {
        template.r#type.clone()
    };

    let display = template
        .display
        .as_ref()
        .map(|d| {
            let logo = d.logo.as_ref().and_then(|logo| {
                logo.uri.parse().ok().map(|uri| OidcLogo {
                    uri,
                    alt_text: logo.alt_text.clone(),
                })
            });
            vec![CredentialConfigurationsSupportedDisplay {
                name: d.name.clone(),
                locale: None,
                logo,
                description: None,
                background_image: None,
                background_color: None,
                text_color: None,
            }]
        })
        .or_else(|| {
            Some(vec![CredentialConfigurationsSupportedDisplay {
                name: template.title.clone(),
                locale: None,
                logo: None,
                description: None,
                background_image: None,
                background_color: None,
                text_color: None,
            }])
        });

    let claims = if format == "vc+sd-jwt" {
        build_claims_from_schema(template, Some("credentialSubject."))
    } else if format == "dc+sd-jwt" {
        build_claims_from_schema(template, None)
    } else {
        None
    };

    CredentialConfiguration {
        credential_configuration_id: template.template_id.clone(),
        format,
        type_,
        credential_metadata: CredentialMetadata { display, claims },
        authorization: template.holder_authorization.clone(),
    }
}

/// Builds claim descriptions from the template schema.
///
/// Nested objects are traversed recursively and only leaf fields become claims.
/// Arrays are exposed as a single claim at the array field itself.
/// Attribute lookup uses dotted keys such as `achievement.criteria.narrative`.
/// A claim defaults to non-mandatory unless attributes explicitly mark it as non-disclosable.
fn build_claims_from_schema(template: &Template, prefix: Option<&str>) -> Option<Vec<ClaimDescription>> {
    let schema = template.schema.as_ref().as_ref()?;
    let attributes = template.schema_properties_attributes.as_ref();
    let prefix_segments: Vec<String> = prefix
        .iter()
        .flat_map(|value| value.split('.').filter(|segment| !segment.is_empty()))
        .map(|segment| segment.to_string())
        .collect();

    let mut claims = Vec::new();
    collect_claim_descriptions(schema, &prefix_segments, &[], attributes, &mut claims);

    if claims.is_empty() {
        None
    } else {
        Some(claims)
    }
}

fn collect_claim_descriptions(
    schema: &serde_json::Value,
    prefix_segments: &[String],
    current_segments: &[String],
    attributes: Option<&HashMap<String, PropertyAttribute>>,
    claims: &mut Vec<ClaimDescription>,
) {
    let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) else {
        return;
    };

    for (property_name, property_schema) in properties {
        let mut property_segments = current_segments.to_vec();
        property_segments.extend(split_path_segments(property_name));

        if has_nested_object_properties(property_schema) {
            collect_claim_descriptions(property_schema, prefix_segments, &property_segments, attributes, claims);
            continue;
        }

        if is_object_schema(property_schema) {
            continue;
        }

        if let Some(claim) = build_claim_description(&property_segments, prefix_segments, attributes) {
            claims.push(claim);
        }
    }
}

fn build_claim_description(
    property_segments: &[String],
    prefix_segments: &[String],
    attributes: Option<&HashMap<String, PropertyAttribute>>,
) -> Option<ClaimDescription> {
    let path = ClaimPathPointer::try_new(
        prefix_segments
            .iter()
            .chain(property_segments.iter())
            .cloned()
            .map(ClaimPathElement::String)
            .collect(),
    )
    .ok()?;

    let attribute_key = property_segments.join(".");
    let mandatory = attributes
        .and_then(|attrs| attrs.get(&attribute_key))
        .map(|attr| !attr.is_selectively_disclosable())
        .unwrap_or(false);

    Some(ClaimDescription {
        path,
        mandatory,
        display: vec![],
    })
}

fn split_path_segments(path: &str) -> Vec<String> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .collect()
}

fn has_nested_object_properties(schema: &serde_json::Value) -> bool {
    schema
        .get("properties")
        .and_then(|value| value.as_object())
        .is_some_and(|properties| !properties.is_empty())
}

fn is_object_schema(schema: &serde_json::Value) -> bool {
    schema.get("type").and_then(|value| value.as_str()) == Some("object")
}

#[async_trait]
impl Query<Template> for CredentialConfigurationProjection {
    async fn dispatch(&self, _aggregate_id: &str, events: &[EventEnvelope<Template>]) {
        use agent_library::template::aggregate::Status;
        use agent_library::template::event::TemplateEvent::*;

        for event in events {
            match &event.payload {
                // On creation we have the full template state in the event itself.
                TemplateCreated {
                    template_id,
                    title,
                    display,
                    data_model,
                    r#type,
                    status,
                    schema,
                    schema_properties_attributes,
                    holder_authorization,
                    ..
                } => {
                    // Only published templates have a credential configuration.
                    if *status != Status::Published {
                        continue;
                    }

                    let template = Template {
                        template_id: template_id.clone(),
                        title: title.clone(),
                        display: *display.clone(),
                        data_model: data_model.clone(),
                        r#type: r#type.clone(),
                        status: status.clone(),
                        schema: schema.clone(),
                        schema_properties_attributes: schema_properties_attributes.clone(),
                        holder_authorization: holder_authorization.clone(),
                        ..Default::default()
                    };

                    let credential_configuration = credential_configuration_from_template(&template);
                    let command = ServerConfigCommand::UpdateCredentialConfiguration {
                        credential_configuration,
                        provisioned: false,
                    };
                    if let Err(e) = command_handler(
                        self.issuance_state.authorization_checker.clone(),
                        None,
                        SERVER_CONFIG_ID,
                        &self.issuance_state.command.server_config,
                        command,
                    )
                    .await
                    {
                        warn!("Failed to update credential configuration for template `{template_id}`: {e}");
                    }
                }

                // When the status changes to Deleted, remove the credential configuration;
                // for any other transition (e.g. Draft → Published) re-query and sync.
                StatusUpdated {
                    template_id, status, ..
                } => {
                    if *status == Status::Deleted {
                        self.remove_credential_configuration(template_id).await;
                        continue;
                    }
                    self.sync_from_view(template_id).await;
                }

                // For partial updates re-query the current template state to rebuild the full
                // credential configuration.
                TitleUpdated { template_id, .. }
                | DisplayUpdated { template_id, .. }
                | TypeUpdated { template_id, .. }
                | SchemaUpdated { template_id, .. }
                | SchemaPropertiesAttributesUpdated { template_id, .. }
                | HolderAuthorizationUpdated { template_id, .. } => {
                    self.sync_from_view(template_id).await;
                }

                // When a template is deleted, remove the corresponding credential configuration.
                TemplateDeleted { template_id } => {
                    self.remove_credential_configuration(template_id).await;
                }

                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_library::template::aggregate::{DataModel, Display};

    #[test]
    fn test_v1_data_model_produces_jwt_vc_json_format() {
        let template = Template {
            template_id: "t1".to_string(),
            data_model: DataModel::W3CVcDataModelV1_1,
            r#type: vec!["VerifiableCredential".to_string()],
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.format, "jwt_vc_json");
    }

    #[test]
    fn test_v2_data_model_produces_vc_sd_jwt_format() {
        let template = Template {
            template_id: "t2".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            r#type: vec!["VerifiableCredential".to_string()],
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.format, "vc+sd-jwt");
    }

    #[test]
    fn test_display_name_takes_precedence_over_title() {
        let template = Template {
            template_id: "t4".to_string(),
            display: Some(Display {
                name: "Display Name".to_string(),
                logo: None,
            }),
            title: "Fallback Title".to_string(),
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        let name = config
            .credential_metadata
            .display
            .as_ref()
            .unwrap()
            .first()
            .unwrap()
            .name
            .clone();
        assert_eq!(name, "Display Name");
    }

    #[test]
    fn test_title_used_as_fallback_display_name() {
        let template = Template {
            template_id: "t5".to_string(),
            display: None,
            title: "My Title".to_string(),
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        let name = config
            .credential_metadata
            .display
            .as_ref()
            .unwrap()
            .first()
            .unwrap()
            .name
            .clone();
        assert_eq!(name, "My Title");
    }

    #[test]
    fn test_vc_sd_jwt_includes_claims_from_schema_properties() {
        use agent_library::template::aggregate::PropertyAttribute;
        use std::collections::HashMap;

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" }
            }
        });

        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), PropertyAttribute::new(true, false));
        attrs.insert("age".to_string(), PropertyAttribute::new(false, false));

        let template = Template {
            template_id: "t7".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(attrs),
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        assert_eq!(config.format, "vc+sd-jwt");

        let claims = config.credential_metadata.claims.expect("claims should be present");
        assert_eq!(claims.len(), 2);

        // Find claim for "name" - selectively disclosable, so mandatory = false
        let name_claim = claims
            .iter()
            .find(|c| {
                c.path.as_ref()
                    == &[
                        ClaimPathElement::String("credentialSubject".to_string()),
                        ClaimPathElement::String("name".to_string()),
                    ]
            })
            .expect("name claim should exist");
        assert!(!name_claim.mandatory);

        // Find claim for "age" - not selectively disclosable, so mandatory = true
        let age_claim = claims
            .iter()
            .find(|c| {
                c.path.as_ref()
                    == &[
                        ClaimPathElement::String("credentialSubject".to_string()),
                        ClaimPathElement::String("age".to_string()),
                    ]
            })
            .expect("age claim should exist");
        assert!(age_claim.mandatory);
    }

    #[test]
    fn test_vc_sd_jwt_claim_without_attributes_defaults_to_non_mandatory() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let template = Template {
            template_id: "t_default_mandatory".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        let claim = config
            .credential_metadata
            .claims
            .expect("claims should be present")
            .into_iter()
            .next()
            .expect("claim should exist");

        assert!(!claim.mandatory);
    }

    #[test]
    fn test_vc_sd_jwt_dotted_keys_become_path_elements() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement.name": { "type": "string" }
            }
        });

        let template = Template {
            template_id: "t8".to_string(),
            data_model: DataModel::OpenBadges3_0,
            schema: Box::new(Some(schema)),
            schema_properties_attributes: None,
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        let claims = config.credential_metadata.claims.expect("claims should be present");
        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].path.as_ref(),
            &[
                ClaimPathElement::String("credentialSubject".to_string()),
                ClaimPathElement::String("achievement".to_string()),
                ClaimPathElement::String("name".to_string()),
            ]
        );
    }

    #[test]
    fn test_nested_object_claims_only_emit_leaf_fields() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
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

        let template = Template {
            template_id: "t_nested".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(Some(schema)),
            ..Default::default()
        };

        let claims = credential_configuration_from_template(&template)
            .credential_metadata
            .claims
            .expect("claims should be present");

        assert_eq!(claims.len(), 2);
        assert!(claims.iter().any(|claim| {
            claim.path.as_ref()
                == &[
                    ClaimPathElement::String("credentialSubject".to_string()),
                    ClaimPathElement::String("achievement".to_string()),
                    ClaimPathElement::String("name".to_string()),
                ]
        }));
        assert!(claims.iter().any(|claim| {
            claim.path.as_ref()
                == &[
                    ClaimPathElement::String("credentialSubject".to_string()),
                    ClaimPathElement::String("achievement".to_string()),
                    ClaimPathElement::String("criteria".to_string()),
                    ClaimPathElement::String("narrative".to_string()),
                ]
        }));
    }

    #[test]
    fn test_array_claims_emit_only_the_array_field() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievements": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                }
            }
        });

        let template = Template {
            template_id: "t_array".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(Some(schema)),
            ..Default::default()
        };

        let claims = credential_configuration_from_template(&template)
            .credential_metadata
            .claims
            .expect("claims should be present");

        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].path.as_ref(),
            &[
                ClaimPathElement::String("credentialSubject".to_string()),
                ClaimPathElement::String("achievements".to_string()),
            ]
        );
    }

    #[test]
    fn test_nested_attributes_use_dotted_keys_for_mandatory_resolution() {
        use agent_library::template::aggregate::PropertyAttribute;
        use std::collections::HashMap;

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "achievement": {
                    "type": "object",
                    "properties": {
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

        let mut attrs = HashMap::new();
        attrs.insert(
            "achievement.criteria.narrative".to_string(),
            PropertyAttribute::new(false, false),
        );

        let template = Template {
            template_id: "t_nested_attrs".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(Some(schema)),
            schema_properties_attributes: Some(attrs),
            ..Default::default()
        };

        let claim = credential_configuration_from_template(&template)
            .credential_metadata
            .claims
            .expect("claims should be present")
            .into_iter()
            .next()
            .expect("claim should exist");

        assert!(claim.mandatory);
    }

    #[test]
    fn test_jwt_vc_json_format_does_not_include_claims() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let template = Template {
            template_id: "t9".to_string(),
            data_model: DataModel::W3CVcDataModelV1_1,
            schema: Box::new(Some(schema)),
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        assert_eq!(config.format, "jwt_vc_json");
        assert!(config.credential_metadata.claims.is_none());
    }

    #[test]
    fn test_vc_sd_jwt_no_schema_yields_no_claims() {
        let template = Template {
            template_id: "t10".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(None),
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        assert!(config.credential_metadata.claims.is_none());
    }

    #[test]
    fn test_w3c_v1_data_model_type_includes_verifiable_credential() {
        let template = Template {
            template_id: "t11".to_string(),
            data_model: DataModel::W3CVcDataModelV1_1,
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.type_, vec!["VerifiableCredential".to_string()]);
    }

    #[test]
    fn test_w3c_v2_data_model_type_includes_verifiable_credential() {
        let template = Template {
            template_id: "t12".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.type_, vec!["VerifiableCredential".to_string()]);
    }

    #[test]
    fn test_non_empty_template_type_is_used_for_projection() {
        let template = Template {
            template_id: "t12_custom".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            r#type: vec![
                "VerifiableCredential".to_string(),
                "UniversityDegreeCredential".to_string(),
            ],
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        assert_eq!(
            config.type_,
            vec![
                "VerifiableCredential".to_string(),
                "UniversityDegreeCredential".to_string(),
            ]
        );
    }

    #[test]
    fn test_vc_sd_jwt_claims_are_prefixed_with_credential_subject() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let template = Template {
            template_id: "t_prefix".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(Some(schema)),
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        let claims = config.credential_metadata.claims.expect("claims should be present");
        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].path.as_ref(),
            &[
                ClaimPathElement::String("credentialSubject".to_string()),
                ClaimPathElement::String("name".to_string()),
            ]
        );
    }

    #[test]
    fn test_open_badges_empty_type_defaults_to_ob_fallback() {
        let template = Template {
            template_id: "t13".to_string(),
            data_model: DataModel::OpenBadges3_0,
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(
            config.type_,
            vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string(),]
        );
    }

    #[test]
    fn test_open_badges_non_empty_template_type_is_used_for_projection() {
        let template = Template {
            template_id: "t13_custom".to_string(),
            data_model: DataModel::OpenBadges3_0,
            r#type: vec!["VerifiableCredential".to_string(), "AchievementCredential".to_string()],
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        assert_eq!(
            config.type_,
            vec!["VerifiableCredential".to_string(), "AchievementCredential".to_string(),]
        );
    }
}
