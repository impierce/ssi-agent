use crate::server_config::command::ServerConfigCommand;
use crate::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_library::template::aggregate::{DataModel, Template};
use agent_library::template::views::TemplateView;
use agent_shared::config::{Authorization, CredentialConfiguration};
use agent_shared::handlers::{command_handler, query_handler};
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use cqrs_es::{EventEnvelope, Query};
use oid4vc_core::claim_path_pointer::{ClaimPathElement, ClaimPathPointer};
use oid4vci::credential_issuer::credential_configurations_supported::{
    ClaimDescription, CredentialConfigurationsSupportedDisplay, CredentialMetadata, Logo as OidcLogo,
};
use std::sync::{Arc, OnceLock};
use tracing::warn;

pub struct IssuerMetadataSynchronizationPolicy {
    issuance_state: Arc<IssuanceState>,
    /// The template view repository used to re-query the current template state on partial updates.
    ///
    /// This is a `OnceLock` so that the real library state's view repository can be injected AFTER
    /// the library state (and hence the CQRS framework) is constructed, avoiding the circular
    /// dependency: policy → library state → policy. By the time any template events arrive the
    /// application is already started and the lock is set.
    template_view: Arc<OnceLock<Arc<dyn ViewRepository<TemplateView, Template>>>>,
}

impl IssuerMetadataSynchronizationPolicy {
    /// Creates a new policy.
    ///
    /// Returns both the policy and the `OnceLock` handle.  After building the library state that
    /// owns this policy, call `handle.set(library_state.query.template.clone()).unwrap()` to wire
    /// the real (shared) view repository into the policy.
    pub fn new(
        issuance_state: Arc<IssuanceState>,
    ) -> (Self, Arc<OnceLock<Arc<dyn ViewRepository<TemplateView, Template>>>>) {
        let template_view = Arc::new(OnceLock::new());
        let policy = Self {
            issuance_state,
            template_view: template_view.clone(),
        };
        (policy, template_view)
    }

    /// Re-queries the current template state from the view repository and, if the template is not
    /// in Draft status, updates (or creates) the corresponding credential configuration.
    async fn sync_from_view(&self, template_id: &str) {
        use agent_library::template::aggregate::Status;

        let Some(view) = self.template_view.get() else {
            warn!("Template view not yet initialized; skipping credential configuration sync for `{template_id}`");
            return;
        };
        match query_handler(template_id, view).await {
            Ok(Some(template)) => {
                if template.status == Status::Draft {
                    return;
                }
                let credential_configuration = credential_configuration_from_template(&template);
                let command = ServerConfigCommand::UpdateCredentialConfiguration {
                    credential_configuration,
                    provisioned: false,
                };
                if let Err(e) =
                    command_handler(SERVER_CONFIG_ID, &self.issuance_state.command.server_config, command).await
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
        if let Err(e) = command_handler(SERVER_CONFIG_ID, &self.issuance_state.command.server_config, command).await {
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
/// The `credential_definition.type` array is determined by the template's data model:
/// - For `w3c_vc_data_model_v1-1` and `w3c_vc_data_model_v2-0`: includes "VerifiableCredential"
/// - For `open_badges_3-0`: uses ["OpenBadgeCredential", "AchievementCredential"]
/// - Otherwise: uses the template's `type` field as-is
fn credential_configuration_from_template(template: &Template) -> CredentialConfiguration {
    let format = match template.data_model {
        Some(DataModel::W3CVcDataModelV1_1) => "jwt_vc_json",
        _ => "vc+sd-jwt",
    }
    .to_string();

    let type_ = match template.data_model {
        Some(DataModel::W3CVcDataModelV1_1) | Some(DataModel::W3CVcDataModelV2_0) => {
            vec!["VerifiableCredential".to_string()]
        }
        Some(DataModel::OpenBadges3_0) => {
            vec!["OpenBadgeCredential".to_string(), "AchievementCredential".to_string()]
        }
        _ => template.r#type.clone(),
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
            template.title.as_ref().map(|title| {
                vec![CredentialConfigurationsSupportedDisplay {
                    name: title.clone(),
                    locale: None,
                    logo: None,
                    description: None,
                    background_image: None,
                    background_color: None,
                    text_color: None,
                }]
            })
        });

    let claims = if format == "vc+sd-jwt" {
        build_claims_from_schema(template)
    } else {
        None
    };

    CredentialConfiguration {
        credential_configuration_id: template.template_id.clone(),
        format,
        type_,
        credential_metadata: CredentialMetadata { display, claims },
        authorization: Authorization::default(),
    }
}

/// Builds claim descriptions from the template's `schema.properties`, enriched with
/// the `selectively_disclosable` flag from `schema_properties_attributes`.
///
/// Each property key (which may be dot-separated, e.g. "achievement.name") is converted
/// into a `ClaimPathPointer` (e.g. `["achievement", "name"]`).
/// A claim is marked as mandatory when it is NOT selectively disclosable.
fn build_claims_from_schema(template: &Template) -> Option<Vec<ClaimDescription>> {
    let schema = template.schema.as_ref().as_ref()?;
    let properties = schema.get("properties")?.as_object()?;

    if properties.is_empty() {
        return None;
    }

    let attributes = template.schema_properties_attributes.as_ref();

    let claims: Vec<ClaimDescription> = properties
        .keys()
        .filter_map(|key| {
            let path_elements: Vec<ClaimPathElement> = key
                .split('.')
                .map(|segment| ClaimPathElement::String(segment.to_string()))
                .collect();

            let path = ClaimPathPointer::try_new(path_elements).ok()?;

            let mandatory = attributes
                .and_then(|attrs| attrs.get(key))
                .map(|attr| !attr.is_selectively_disclosable())
                .unwrap_or(false);

            Some(ClaimDescription {
                path,
                mandatory,
                display: vec![],
            })
        })
        .collect();

    if claims.is_empty() {
        None
    } else {
        Some(claims)
    }
}

#[async_trait]
impl Query<Template> for IssuerMetadataSynchronizationPolicy {
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
                    ..
                } => {
                    // Only register a credential configuration once the template has left the draft stage.
                    if *status == Status::Draft || *status == Status::Deleted {
                        continue;
                    }

                    let template = Template {
                        template_id: template_id.clone(),
                        title: title.clone(),
                        display: *display.clone(),
                        data_model: data_model.clone(),
                        r#type: r#type.clone(),
                        status: status.clone(),
                        ..Default::default()
                    };

                    let credential_configuration = credential_configuration_from_template(&template);
                    let command = ServerConfigCommand::UpdateCredentialConfiguration {
                        credential_configuration,
                        provisioned: false,
                    };
                    if let Err(e) =
                        command_handler(SERVER_CONFIG_ID, &self.issuance_state.command.server_config, command).await
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
                | DataModelUpdated { template_id, .. }
                | TypeUpdated { template_id, .. } => {
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
            data_model: Some(DataModel::W3CVcDataModelV1_1),
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
            data_model: Some(DataModel::W3CVcDataModelV2_0),
            r#type: vec!["VerifiableCredential".to_string()],
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.format, "vc+sd-jwt");
    }

    #[test]
    fn test_absent_data_model_produces_vc_sd_jwt_format() {
        let template = Template {
            template_id: "t3".to_string(),
            data_model: None,
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
            title: Some("Fallback Title".to_string()),
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
            title: Some("My Title".to_string()),
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
    fn test_no_display_no_title_yields_no_credential_display() {
        let template = Template {
            template_id: "t6".to_string(),
            display: None,
            title: None,
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert!(config.credential_metadata.display.is_none());
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
            data_model: Some(DataModel::W3CVcDataModelV2_0),
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
            .find(|c| c.path.as_ref() == &[ClaimPathElement::String("name".to_string())])
            .expect("name claim should exist");
        assert!(!name_claim.mandatory);

        // Find claim for "age" - not selectively disclosable, so mandatory = true
        let age_claim = claims
            .iter()
            .find(|c| c.path.as_ref() == &[ClaimPathElement::String("age".to_string())])
            .expect("age claim should exist");
        assert!(age_claim.mandatory);
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
            data_model: Some(DataModel::OpenBadges3_0),
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
                ClaimPathElement::String("achievement".to_string()),
                ClaimPathElement::String("name".to_string()),
            ]
        );
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
            data_model: Some(DataModel::W3CVcDataModelV1_1),
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
            data_model: Some(DataModel::W3CVcDataModelV2_0),
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
            data_model: Some(DataModel::W3CVcDataModelV1_1),
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.type_, vec!["VerifiableCredential".to_string()]);
    }

    #[test]
    fn test_w3c_v2_data_model_type_includes_verifiable_credential() {
        let template = Template {
            template_id: "t12".to_string(),
            data_model: Some(DataModel::W3CVcDataModelV2_0),
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.type_, vec!["VerifiableCredential".to_string()]);
    }

    #[test]
    fn test_open_badges_data_model_type_uses_ob_types() {
        let template = Template {
            template_id: "t13".to_string(),
            data_model: Some(DataModel::OpenBadges3_0),
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(
            config.type_,
            vec!["OpenBadgeCredential".to_string(), "AchievementCredential".to_string(),]
        );
    }

    #[test]
    fn test_absent_data_model_uses_template_type_field() {
        let template = Template {
            template_id: "t14".to_string(),
            data_model: None,
            r#type: vec!["CustomType".to_string()],
            ..Default::default()
        };
        let config = credential_configuration_from_template(&template);
        assert_eq!(config.type_, vec!["CustomType".to_string()]);
    }
}
