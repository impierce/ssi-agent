use crate::server_config::command::ServerConfigCommand;
use crate::server_config::views::ServerConfigView;
use crate::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{
    is_array_schema, DataModel, Display, FormFieldType, HolderType, PropertyAttribute, Template,
};
use agent_library::template::views::TemplateView;
use agent_shared::config::CredentialConfiguration;
use agent_shared::handlers::{command_handler, public_query_handler};
use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};
use oid4vc_core::claim_path_pointer::{ClaimPathElement, ClaimPathPointer};
use oid4vci::credential_issuer::credential_configurations_supported::{
    ClaimDescription, ClaimDescriptionDisplay, CredentialConfigurationsSupportedDisplay, CredentialMetadata,
    Logo as OidcLogo,
};
use shared_kernel::authorization::Caller;
use shared_kernel::view_repository::DynViewRepository;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
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
            Ok(Some(template)) => match template.status {
                Status::Published => {
                    self.update_credential_configuration(
                        template_id,
                        credential_configuration_from_template(&template),
                    )
                    .await;
                }
                Status::Draft => self.remove_listed_credential_configuration(template_id).await,
                Status::Archived | Status::Deleted => self.remove_credential_configuration(template_id).await,
            },
            Ok(None) => {
                warn!("Template `{template_id}` not found when trying to sync credential configuration");
            }
            Err(e) => {
                warn!("Failed to query template `{template_id}` for credential configuration sync: {e}");
            }
        }
    }

    /// Adds or replaces the credential configuration of a template, unless the server config already
    /// holds and advertises exactly this configuration.
    async fn update_credential_configuration(
        &self,
        template_id: &str,
        credential_configuration: CredentialConfiguration,
    ) {
        if let Some(server_config) = self.server_config_view().await {
            let unchanged = server_config.advertises_credential_configurations()
                && server_config
                    .with_credential_configuration(credential_configuration.clone(), false)
                    .is_ok_and(|credential_configurations| {
                        credential_configurations == server_config.credential_configurations
                    });
            if unchanged {
                return;
            }
        }

        let command = ServerConfigCommand::UpdateCredentialConfiguration {
            credential_configuration,
            provisioned: false,
        };
        if let Err(e) = command_handler(
            self.issuance_state.authorization_checker.clone(),
            Caller::Internal,
            SERVER_CONFIG_ID,
            &self.issuance_state.command.server_config,
            command,
        )
        .await
        {
            warn!("Failed to update credential configuration for template `{template_id}`: {e}");
        }
    }

    /// The server config read model. Commands on the server config are checked against it first,
    /// since a command loads the aggregate, which replays every server config event, each carrying
    /// all credential configurations. The aggregate records every command it gets, so a view that
    /// lost a write is repaired by the next sync of the template.
    async fn server_config_view(&self) -> Option<ServerConfigView> {
        public_query_handler(SERVER_CONFIG_ID, &self.issuance_state.query.server_config)
            .await
            .ok()
            .flatten()
    }

    /// Removes the credential configuration of a draft template, unless the server config does not
    /// list it. Drafts sync on every edit, and a draft was never published, so this skips a server
    /// config command per edit.
    async fn remove_listed_credential_configuration(&self, template_id: &str) {
        if self.server_config_view().await.is_some_and(|server_config| {
            server_config.advertises_credential_configurations()
                && !server_config.credential_configurations.contains_key(template_id)
        }) {
            return;
        }

        self.remove_credential_configuration(template_id).await;
    }

    /// Removes the credential configuration associated with the given template ID.
    ///
    /// Archiving and deleting do not check the server config view: it can still lack the
    /// configuration of a publish whose view write has not landed yet, and an archived or deleted
    /// template gets no later sync that would remove it.
    async fn remove_credential_configuration(&self, template_id: &str) {
        let command = ServerConfigCommand::RemoveCredentialConfiguration {
            credential_configuration_id: template_id.to_string(),
            provisioned: false,
        };
        if let Err(e) = command_handler(
            self.issuance_state.authorization_checker.clone(),
            Caller::Internal,
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

/// Derives the credential configuration of every published template again.
///
/// A configuration is otherwise derived only when its template changes, so one that an earlier version derived
/// differently would be advertised as it was until then. A configuration that is already current is left alone.
pub async fn resync_credential_configurations(issuance_state: Arc<IssuanceState>, library_state: &LibraryState) {
    use agent_library::template::aggregate::Status;

    let all_templates = match public_query_handler("all_templates", &library_state.query.all_templates).await {
        Ok(Some(all_templates)) => all_templates,
        Ok(None) => return,
        Err(e) => {
            warn!("Failed to query the templates to resync their credential configurations: {e}");
            return;
        }
    };

    let (projection, _) = CredentialConfigurationProjection::new(issuance_state);
    for (template_id, template) in &all_templates.templates {
        if template.status == Status::Published {
            projection
                .update_credential_configuration(template_id, credential_configuration_from_template(template))
                .await;
        }
    }
}

/// Derives a `CredentialConfiguration` from a `Template`.
///
/// The display name is taken from `template.display.name` if set, falling back to `template.title`.
/// An empty (or whitespace-only) `display.name` counts as unset: `Display.name` cannot currently
/// express "not provided", so clients that submit a `display` object before a title is known (for
/// instance when uploading a logo first) end up storing `""`. Resolving the fallback here rather
/// than materializing it into the aggregate keeps the link live: as long as no explicit display
/// name was given, later title updates keep flowing into the issuer metadata.
/// When the format is "vc+sd-jwt", claims are derived from `schema.properties`, and are mandatory where the
/// schema requires them.
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

    let resolved_display = Display::resolve(template.display.clone(), &template.title);
    let logo = resolved_display.logo.as_ref().and_then(|logo| {
        logo.uri.parse().ok().map(|uri| OidcLogo {
            uri,
            alt_text: logo.alt_text.clone(),
        })
    });
    let display = Some(vec![CredentialConfigurationsSupportedDisplay {
        name: resolved_display.name,
        locale: None,
        logo,
        description: None,
        background_image: None,
        background_color: None,
        text_color: None,
    }]);

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

/// The `format` of a value claim, from the schema `format` of its property.
const VALUE_FORMATS: [&str; 4] = ["country", "date", "date-time", "uri"];

/// Builds the claims descriptions of the template schema (OpenID4VCI 1.0, Appendix B.2), in the order of the schema's
/// `properties`, which a Wallet takes as the display order (Appendix B.3).
///
/// - Every property has a description, with its schema `title` as display name. An Object's description comes before
///   those of its properties.
/// - An array has a description of its own. The properties of its items are described with `null` at the array's
///   position (Appendix C.1). Items that are values are described only when they carry a `format`.
/// - A display object carries a `format` for a Table (`table`), and for a value whose type a Wallet can show in a
///   better way (see [`VALUE_FORMATS`]).
/// - A claim is mandatory when every credential of the template holds it: the schema lists its property in
///   `required`, and likewise every object above it. Through an array, the array has to hold at least one item
///   (`minItems`). Whether a claim is selectively disclosable plays no part in that.
fn build_claims_from_schema(template: &Template, prefix: Option<&str>) -> Option<Vec<ClaimDescription>> {
    let schema = template.schema.as_ref().as_ref()?;
    let prefix: Vec<ClaimPathElement> = prefix
        .iter()
        .flat_map(|value| value.split('.').filter(|segment| !segment.is_empty()))
        .map(|segment| ClaimPathElement::String(segment.to_string()))
        .collect();

    let mut builder = ClaimsBuilder {
        attributes: template.schema_properties_attributes.as_ref(),
        claims: Vec::new(),
    };
    builder.describe_properties(schema, &prefix, Some(""), true);

    if builder.claims.is_empty() {
        None
    } else {
        Some(builder.claims)
    }
}

struct ClaimsBuilder<'a> {
    attributes: Option<&'a HashMap<String, PropertyAttribute>>,
    claims: Vec<ClaimDescription>,
}

impl ClaimsBuilder<'_> {
    /// Describes the properties of the object `schema` at `path`.
    ///
    /// `pointer` is the JSON Pointer of `schema` that `schemaPropertiesAttributes` keys use, `None` inside an array:
    /// a key never leads into `items`. `always_present` is whether every credential holds an object at `path`.
    fn describe_properties(
        &mut self,
        schema: &serde_json::Value,
        path: &[ClaimPathElement],
        pointer: Option<&str>,
        always_present: bool,
    ) {
        let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) else {
            return;
        };
        let required: Vec<&str> = schema
            .get("required")
            .and_then(|value| value.as_array())
            .map(|names| names.iter().filter_map(|name| name.as_str()).collect())
            .unwrap_or_default();

        for (property_name, property_schema) in properties {
            // A property name is one path element, whatever characters it holds.
            let mut property_path = path.to_vec();
            property_path.push(ClaimPathElement::String(property_name.clone()));
            // RFC 6901: escape `~` as `~0` and `/` as `~1`.
            let property_pointer =
                pointer.map(|pointer| format!("{pointer}/{}", property_name.replace('~', "~0").replace('/', "~1")));
            let mandatory = always_present && required.contains(&property_name.as_str());

            self.describe(
                property_schema,
                property_path,
                property_pointer.as_deref(),
                mandatory,
                true,
            );
        }
    }

    /// Describes the value of `schema` at `path`, then what it holds. Without `always_describe`, a value is described
    /// only when its display object has a `format`.
    fn describe(
        &mut self,
        schema: &serde_json::Value,
        path: Vec<ClaimPathElement>,
        pointer: Option<&str>,
        mandatory: bool,
        always_describe: bool,
    ) {
        let format = self.format(schema, pointer);
        if always_describe || format.is_some() {
            let name = schema
                .get("title")
                .and_then(|title| title.as_str())
                .filter(|title| !title.trim().is_empty());
            self.push(&path, mandatory, name, format);
        }

        // An array holds items, also when its schema carries `properties`, which say nothing about an array.
        if is_array_schema(schema) {
            let Some(items) = schema.get("items") else {
                return;
            };
            // Every item is there only if the array has to be one and holds at least one item.
            let has_items = mandatory
                && has_only_type(schema, "array")
                && schema
                    .get("minItems")
                    .and_then(|min_items| min_items.as_u64())
                    .is_some_and(|min_items| min_items >= 1);
            let mut items_path = path;
            items_path.push(ClaimPathElement::Null);
            self.describe(items, items_path, None, has_items, false);
        } else if has_nested_object_properties(schema) {
            // What is inside is always there only if the value has to be an object: `null`, or any value where no
            // type is given, satisfies the schema without it.
            let always_present = mandatory && has_only_type(schema, "object");
            self.describe_properties(schema, &path, pointer, always_present);
        }
    }

    /// The `format` of the display object of the value of `schema`: `table` for a Table, one of [`VALUE_FORMATS`] for
    /// a value. Objects, Lists and values without one of these formats need none: the data shows what they are.
    fn format(&self, schema: &serde_json::Value, pointer: Option<&str>) -> Option<&'static str> {
        let schema_format = schema.get("format").and_then(|format| format.as_str());
        if is_array_schema(schema) {
            return (schema_format == Some("table")).then_some("table");
        }
        if is_object_schema(schema) {
            return None;
        }
        if let Some(format) = VALUE_FORMATS.iter().find(|format| Some(**format) == schema_format) {
            return Some(format);
        }
        let attribute_type = pointer
            .and_then(|pointer| self.attributes?.get(pointer))
            .and_then(|attribute| attribute.r#type);
        (attribute_type == Some(FormFieldType::Country)).then_some("country")
    }

    fn push(&mut self, path: &[ClaimPathElement], mandatory: bool, name: Option<&str>, format: Option<&str>) {
        let Ok(path) = ClaimPathPointer::try_new(path.to_vec()) else {
            return;
        };
        let display = if name.is_none() && format.is_none() {
            vec![]
        } else {
            vec![ClaimDescriptionDisplay {
                name: name.map(str::to_string),
                locale: None,
                additional_parameters: format
                    .map(|format| serde_json::Map::from_iter([("format".to_string(), format.into())]))
                    .unwrap_or_default(),
            }]
        };

        self.claims.push(ClaimDescription {
            path,
            mandatory,
            display,
        });
    }
}

fn has_nested_object_properties(schema: &serde_json::Value) -> bool {
    schema
        .get("properties")
        .and_then(|value| value.as_object())
        .is_some_and(|properties| !properties.is_empty())
}

/// Whether the `type` of this schema includes `"object"`.
fn is_object_schema(schema: &serde_json::Value) -> bool {
    match schema.get("type") {
        Some(serde_json::Value::String(r#type)) => r#type == "object",
        Some(serde_json::Value::Array(types)) => types.iter().any(|r#type| r#type.as_str() == Some("object")),
        _ => false,
    }
}

/// Whether a value of this schema has to be of this type: its `type` is this one and nothing else.
fn has_only_type(schema: &serde_json::Value, expected: &str) -> bool {
    match schema.get("type") {
        Some(serde_json::Value::String(r#type)) => r#type == expected,
        Some(serde_json::Value::Array(types)) => {
            !types.is_empty() && types.iter().all(|r#type| r#type.as_str() == Some(expected))
        }
        _ => false,
    }
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
                    origin: _,
                } => {
                    // Only published templates have a credential configuration.
                    if *status != Status::Published {
                        continue;
                    }

                    let template = Template {
                        template_id: template_id.clone(),
                        source_template_id: source_template_id.clone(),
                        title: title.clone(),
                        display: *display.clone(),
                        data_model: data_model.clone(),
                        holder_type: holder_type.clone(),
                        modified_at: Some(modified_at.clone()),
                        tags: tags.clone(),
                        status: status.clone(),
                        visibility: visibility.clone(),
                        credential_expiration: credential_expiration.clone(),
                        description: description.clone(),
                        r#type: r#type.clone(),
                        schema: schema.clone(),
                        schema_properties_attributes: schema_properties_attributes.clone(),
                        holder_authorization: holder_authorization.clone(),
                        origin: None,
                    };

                    self.update_credential_configuration(
                        template_id,
                        credential_configuration_from_template(&template),
                    )
                    .await;
                }

                // When the status changes to Archived or Deleted, remove the credential configuration;
                // for any other transition (e.g. Draft → Published) re-query and sync.
                StatusUpdated {
                    template_id, status, ..
                } => {
                    if matches!(status, Status::Archived | Status::Deleted) {
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
    use agent_library::template::aggregate::{DataModel, Display, Logo};

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
    fn test_title_used_as_fallback_when_display_name_is_empty() {
        for name in ["", "   "] {
            let template = Template {
                template_id: "t6".to_string(),
                display: Some(Display {
                    name: name.to_string(),
                    logo: Some(Logo {
                        uri: "https://example.com/logo.png".to_string(),
                        alt_text: None,
                    }),
                }),
                title: "My Title".to_string(),
                ..Default::default()
            };
            let config = credential_configuration_from_template(&template);
            let display = config.credential_metadata.display.as_ref().unwrap().first().unwrap();

            assert_eq!(display.name, "My Title");
            // The rest of the display object is unaffected by the fallback.
            assert_eq!(
                display.logo.as_ref().map(|logo| logo.uri.to_string()),
                Some("https://example.com/logo.png".to_string())
            );
        }
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
            },
            "required": ["age"]
        });

        // Whether a claim is selectively disclosable says nothing about whether it is always there.
        let mut attrs = HashMap::new();
        attrs.insert("/name".to_string(), PropertyAttribute::new(false, false));
        attrs.insert("/age".to_string(), PropertyAttribute::new(true, false));

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

        // "name" is not required by the schema, so mandatory = false
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

        // "age" is required by the schema, so mandatory = true
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
    fn test_vc_sd_jwt_claim_not_required_by_schema_is_not_mandatory() {
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

    /// The claims descriptions of a `vc+sd-jwt` template with this schema and these attributes.
    fn claims_of(
        schema: serde_json::Value,
        attributes: Option<HashMap<String, PropertyAttribute>>,
    ) -> Vec<ClaimDescription> {
        let template = Template {
            template_id: "t_claims".to_string(),
            data_model: DataModel::W3CVcDataModelV2_0,
            schema: Box::new(Some(schema)),
            schema_properties_attributes: attributes,
            ..Default::default()
        };
        credential_configuration_from_template(&template)
            .credential_metadata
            .claims
            .expect("claims should be present")
    }

    fn paths(claims: &[ClaimDescription]) -> Vec<serde_json::Value> {
        claims.iter().map(|claim| serde_json::json!(claim.path)).collect()
    }

    /// The credential holds one key named `No. of credits`, so the path to it has one element for it.
    #[test]
    fn test_property_name_with_dots_is_one_path_element() {
        let claims = claims_of(
            serde_json::json!({
                "type": "object",
                "properties": {
                    "No. of credits": { "type": "string" },
                    "grades": {
                        "type": "object",
                        "properties": { "v1.2": { "type": "string" } }
                    }
                }
            }),
            None,
        );

        assert_eq!(
            paths(&claims),
            vec![
                serde_json::json!(["credentialSubject", "No. of credits"]),
                serde_json::json!(["credentialSubject", "grades"]),
                serde_json::json!(["credentialSubject", "grades", "v1.2"]),
            ]
        );
    }

    #[test]
    fn test_object_is_described_before_its_properties() {
        let claims = claims_of(
            serde_json::json!({
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
            }),
            None,
        );

        assert_eq!(
            paths(&claims),
            vec![
                serde_json::json!(["credentialSubject", "achievement"]),
                serde_json::json!(["credentialSubject", "achievement", "name"]),
                serde_json::json!(["credentialSubject", "achievement", "criteria"]),
                serde_json::json!(["credentialSubject", "achievement", "criteria", "narrative"]),
            ]
        );
    }

    #[test]
    fn test_properties_of_array_items_are_described_through_null() {
        let claims = claims_of(
            serde_json::json!({
                "type": "object",
                "properties": {
                    "achievements": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "tasks": { "type": "array", "items": { "type": "string", "format": "uri" } }
                            }
                        }
                    }
                }
            }),
            None,
        );

        assert_eq!(
            paths(&claims),
            vec![
                serde_json::json!(["credentialSubject", "achievements"]),
                serde_json::json!(["credentialSubject", "achievements", null, "name"]),
                serde_json::json!(["credentialSubject", "achievements", null, "tasks"]),
                serde_json::json!(["credentialSubject", "achievements", null, "tasks", null]),
            ]
        );
    }

    /// The training record of issue #534, verbatim.
    #[test]
    fn test_claims_describe_objects_labels_and_value_formats() {
        let claims = claims_of(
            serde_json::json!({
              "type": "object",
              "properties": {
                "Trainee": {
                  "title": "Trainee",
                  "type": "object",
                  "properties": {
                    "Full name": { "title": "Full name", "type": "string" },
                    "Nationality": { "title": "Nationality", "type": "string", "format": "country" }
                  },
                  "required": ["Full name"]
                },
                "Skills": { "title": "Skills", "type": "array", "items": { "type": "string" } },
                "Websites": { "title": "Websites", "type": "array", "items": { "type": "string", "format": "uri" } },
                "Courses": {
                  "title": "Courses",
                  "type": "array",
                  "format": "table",
                  "minItems": 1,
                  "items": {
                    "type": "object",
                    "properties": {
                      "Code": { "title": "Code", "type": "string" },
                      "Title": { "title": "Title", "type": "string" },
                      "Country": { "title": "Country", "type": "string", "format": "country" },
                      "Completed on": { "title": "Completed on", "type": "string", "format": "date" }
                    },
                    "required": ["Code", "Title"]
                  }
                }
              },
              "required": ["Trainee", "Courses"]
            }),
            None,
        );

        let expected: Vec<ClaimDescription> = serde_json::from_value(serde_json::json!([
          { "path": ["credentialSubject", "Trainee"], "mandatory": true, "display": [{ "name": "Trainee" }] },
          { "path": ["credentialSubject", "Trainee", "Full name"], "mandatory": true, "display": [{ "name": "Full name" }] },
          { "path": ["credentialSubject", "Trainee", "Nationality"], "display": [{ "name": "Nationality", "format": "country" }] },
          { "path": ["credentialSubject", "Skills"], "display": [{ "name": "Skills" }] },
          { "path": ["credentialSubject", "Websites"], "display": [{ "name": "Websites" }] },
          { "path": ["credentialSubject", "Websites", null], "display": [{ "format": "uri" }] },
          { "path": ["credentialSubject", "Courses"], "mandatory": true, "display": [{ "name": "Courses", "format": "table" }] },
          { "path": ["credentialSubject", "Courses", null, "Code"], "mandatory": true, "display": [{ "name": "Code" }] },
          { "path": ["credentialSubject", "Courses", null, "Title"], "mandatory": true, "display": [{ "name": "Title" }] },
          { "path": ["credentialSubject", "Courses", null, "Country"], "display": [{ "name": "Country", "format": "country" }] },
          { "path": ["credentialSubject", "Courses", null, "Completed on"], "display": [{ "name": "Completed on", "format": "date" }] }
        ]))
        .unwrap();
        assert_eq!(claims, expected);
    }

    /// The claims follow the order of `properties` as the schema was written, not the order of the property names.
    #[test]
    fn test_claims_follow_the_order_of_the_schema_properties() {
        let schema: serde_json::Value = serde_json::from_str(
            r#"{
                "type": "object",
                "properties": {
                    "Zeta": { "type": "string" },
                    "Alpha": {
                        "type": "object",
                        "properties": { "Two": { "type": "string" }, "One": { "type": "string" } }
                    },
                    "Mid": { "type": "string" }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            paths(&claims_of(schema, None)),
            vec![
                serde_json::json!(["credentialSubject", "Zeta"]),
                serde_json::json!(["credentialSubject", "Alpha"]),
                serde_json::json!(["credentialSubject", "Alpha", "Two"]),
                serde_json::json!(["credentialSubject", "Alpha", "One"]),
                serde_json::json!(["credentialSubject", "Mid"]),
            ]
        );
    }

    /// A display object holds what the schema gives: a `title` as name, and only the formats a Wallet knows.
    #[test]
    fn test_display_holds_title_and_known_formats_only() {
        let claims = claims_of(
            serde_json::json!({
                "type": "object",
                "properties": {
                    "untitled": { "type": "string" },
                    "blank title": { "title": " ", "type": "string" },
                    "email": { "title": "Email", "type": "string", "format": "email" },
                    "born": { "type": "string", "format": "date-time" },
                    "group": { "type": "object", "format": "table", "properties": { "a": { "type": "string" } } },
                    "list": { "title": "List", "type": "array", "format": "uri", "items": { "type": "string" } }
                }
            }),
            None,
        );
        let display = |path: serde_json::Value| -> serde_json::Value {
            let claim = claims
                .iter()
                .find(|claim| serde_json::json!(claim.path) == path)
                .expect("claim should exist");
            serde_json::json!(claim.display)
        };

        assert_eq!(
            display(serde_json::json!(["credentialSubject", "untitled"])),
            serde_json::json!([])
        );
        assert_eq!(
            display(serde_json::json!(["credentialSubject", "blank title"])),
            serde_json::json!([])
        );
        assert_eq!(
            display(serde_json::json!(["credentialSubject", "email"])),
            serde_json::json!([{ "name": "Email" }])
        );
        assert_eq!(
            display(serde_json::json!(["credentialSubject", "born"])),
            serde_json::json!([{ "format": "date-time" }])
        );
        assert_eq!(
            display(serde_json::json!(["credentialSubject", "group"])),
            serde_json::json!([])
        );
        assert_eq!(
            display(serde_json::json!(["credentialSubject", "list"])),
            serde_json::json!([{ "name": "List" }])
        );
    }

    /// A value whose `schemaPropertiesAttributes` type is `country` is a country, also when its schema has no `format`.
    #[test]
    fn test_country_attribute_gives_country_format() {
        let country = PropertyAttribute {
            r#type: Some(FormFieldType::Country),
            ..PropertyAttribute::new(false, false)
        };
        let attributes = HashMap::from([
            ("/Nationality".to_string(), country.clone()),
            ("/Address/Land~1Region".to_string(), country),
        ]);
        let claims = claims_of(
            serde_json::json!({
                "type": "object",
                "properties": {
                    "Nationality": { "type": "string" },
                    "Address": {
                        "type": "object",
                        "properties": {
                            "Land/Region": { "type": "string" },
                            "City": { "type": "string" }
                        }
                    }
                }
            }),
            Some(attributes),
        );

        let formats: Vec<serde_json::Value> = claims
            .iter()
            .map(|claim| serde_json::json!(claim.display.first().map(|display| &display.additional_parameters)))
            .collect();
        assert_eq!(
            formats,
            vec![
                serde_json::json!({ "format": "country" }),
                serde_json::Value::Null,
                serde_json::json!({ "format": "country" }),
                serde_json::Value::Null,
            ]
        );
    }

    /// A nested claim is always there only when every level down to it is required.
    #[rstest::rstest]
    #[case::required_at_every_level(&["achievement"], &["criteria"], &["narrative"], true)]
    #[case::leaf_optional(&["achievement"], &["criteria"], &[], false)]
    #[case::object_in_between_optional(&["achievement"], &[], &["narrative"], false)]
    #[case::top_level_object_optional(&[], &["criteria"], &["narrative"], false)]
    fn test_nested_claim_is_mandatory_when_required_at_every_level(
        #[case] required_at_root: &[&str],
        #[case] required_in_achievement: &[&str],
        #[case] required_in_criteria: &[&str],
        #[case] mandatory: bool,
    ) {
        let claims = claims_of(
            serde_json::json!({
                "type": "object",
                "required": required_at_root,
                "properties": {
                    "achievement": {
                        "type": "object",
                        "required": required_in_achievement,
                        "properties": {
                            "criteria": {
                                "type": "object",
                                "required": required_in_criteria,
                                "properties": {
                                    "narrative": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            }),
            None,
        );

        let narrative = claims.last().expect("claim should exist");
        assert_eq!(
            serde_json::json!(narrative.path),
            serde_json::json!(["credentialSubject", "achievement", "criteria", "narrative"])
        );
        assert_eq!(narrative.mandatory, mandatory);
    }

    /// A required property that may be something else than an object, such as `null`, need not hold what the schema
    /// requires of an object.
    #[rstest::rstest]
    #[case::object(serde_json::json!("object"), true)]
    #[case::object_in_type_list(serde_json::json!(["object"]), true)]
    #[case::nullable_object(serde_json::json!(["object", "null"]), false)]
    #[case::no_type(serde_json::Value::Null, false)]
    fn test_claim_in_required_property_is_mandatory_only_in_an_object(
        #[case] address_type: serde_json::Value,
        #[case] mandatory: bool,
    ) {
        let mut schema = serde_json::json!({
            "type": "object",
            "required": ["address"],
            "properties": {
                "address": {
                    "type": address_type,
                    "required": ["street"],
                    "properties": { "street": { "type": "string" } }
                }
            }
        });
        if schema["properties"]["address"]["type"].is_null() {
            schema["properties"]["address"].as_object_mut().unwrap().remove("type");
        }

        let claims = claims_of(schema, None);

        assert_eq!(claims.len(), 2);
        // The property itself is always there: the schema requires it.
        assert!(claims[0].mandatory);
        assert_eq!(claims[1].mandatory, mandatory);
    }

    /// Through an array, a claim is always there only when the array is, with at least one item, and the items require
    /// it.
    #[rstest::rstest]
    #[case::array_required_with_items(serde_json::json!("array"), &["Courses"], Some(1), &["Code"], true)]
    #[case::array_optional(serde_json::json!("array"), &[], Some(1), &["Code"], false)]
    #[case::array_may_be_empty(serde_json::json!("array"), &["Courses"], Some(0), &["Code"], false)]
    #[case::no_min_items(serde_json::json!("array"), &["Courses"], None, &["Code"], false)]
    #[case::item_property_optional(serde_json::json!("array"), &["Courses"], Some(1), &[], false)]
    #[case::nullable_array(serde_json::json!(["array", "null"]), &["Courses"], Some(1), &["Code"], false)]
    fn test_claim_through_array_is_mandatory_when_always_in_every_item(
        #[case] courses_type: serde_json::Value,
        #[case] required_at_root: &[&str],
        #[case] min_items: Option<u64>,
        #[case] required_in_items: &[&str],
        #[case] mandatory: bool,
    ) {
        let items_always_there = !required_at_root.is_empty()
            && courses_type == serde_json::json!("array")
            && min_items.is_some_and(|min_items| min_items >= 1);
        let mut courses = serde_json::json!({
            "type": courses_type,
            "items": {
                "type": "object",
                "required": required_in_items,
                "properties": { "Code": { "type": "string" } }
            }
        });
        let mut websites = serde_json::json!({
            "type": courses["type"],
            "items": { "type": "string", "format": "uri" }
        });
        if let Some(min_items) = min_items {
            courses["minItems"] = min_items.into();
            websites["minItems"] = min_items.into();
        }

        let claims = claims_of(
            serde_json::json!({
                "type": "object",
                "required": required_at_root.iter().flat_map(|name| [*name, "Websites"]).collect::<Vec<_>>(),
                "properties": { "Courses": courses, "Websites": websites }
            }),
            None,
        );

        assert_eq!(
            paths(&claims),
            vec![
                serde_json::json!(["credentialSubject", "Courses"]),
                serde_json::json!(["credentialSubject", "Courses", null, "Code"]),
                serde_json::json!(["credentialSubject", "Websites"]),
                serde_json::json!(["credentialSubject", "Websites", null]),
            ]
        );
        assert_eq!(claims[1].mandatory, mandatory);
        // The items of a List of values are there whenever the List holds at least one.
        assert_eq!(claims[3].mandatory, items_always_there);
    }

    /// The leaves of nested objects, the objects, the lists and tables and what their items hold, in schema order.
    #[test]
    fn test_structured_schema_claims() {
        use agent_library::template::aggregate::test_utils::{
            structured_schema, structured_schema_properties_attributes,
        };

        let claims = claims_of(structured_schema(), Some(structured_schema_properties_attributes()));

        let expected: Vec<ClaimDescription> = serde_json::from_value(serde_json::json!([
          { "path": ["credentialSubject", "Full name"], "mandatory": true, "display": [{ "name": "Full name" }] },
          { "path": ["credentialSubject", "Address"], "mandatory": true, "display": [{ "name": "Address" }] },
          { "path": ["credentialSubject", "Address", "Street"], "mandatory": true, "display": [{ "name": "Street" }] },
          { "path": ["credentialSubject", "Address", "City"], "display": [{ "name": "City" }] },
          { "path": ["credentialSubject", "Address", "Country"], "display": [{ "name": "Country", "format": "country" }] },
          { "path": ["credentialSubject", "Address", "Coordinates"], "display": [{ "name": "Coordinates" }] },
          { "path": ["credentialSubject", "Address", "Coordinates", "Latitude"], "display": [{ "name": "Latitude" }] },
          { "path": ["credentialSubject", "Address", "Coordinates", "Longitude"], "display": [{ "name": "Longitude" }] },
          { "path": ["credentialSubject", "Skills"], "mandatory": true, "display": [{ "name": "Skills" }] },
          { "path": ["credentialSubject", "Accreditations"], "display": [{ "name": "Accreditations" }] },
          { "path": ["credentialSubject", "Accreditations", null], "display": [{ "format": "uri" }] },
          { "path": ["credentialSubject", "Work experience"], "display": [{ "name": "Work experience" }] },
          { "path": ["credentialSubject", "Work experience", null, "Employer"], "display": [{ "name": "Employer" }] },
          { "path": ["credentialSubject", "Work experience", null, "Since"], "display": [{ "name": "Since", "format": "date" }] },
          { "path": ["credentialSubject", "Work experience", null, "Roles"], "display": [{ "name": "Roles" }] },
          { "path": ["credentialSubject", "Completed classes"], "mandatory": true, "display": [{ "name": "Completed classes", "format": "table" }] },
          { "path": ["credentialSubject", "Completed classes", null, "Code"], "display": [{ "name": "Code" }] },
          { "path": ["credentialSubject", "Completed classes", null, "Name"], "display": [{ "name": "Name" }] },
          { "path": ["credentialSubject", "Completed classes", null, "Passed on"], "display": [{ "name": "Passed on", "format": "date" }] },
          { "path": ["credentialSubject", "Credits"], "display": [{ "name": "Credits" }] }
        ]))
        .unwrap();
        assert_eq!(claims, expected);
    }

    /// The claims descriptions of every data model never break the rules of OpenID4VCI 1.0, Appendix B.3.
    #[rstest::rstest]
    fn test_claims_descriptions_pass_appendix_b3(
        #[values(
            DataModel::W3CVcDataModelV1_1,
            DataModel::W3CVcDataModelV2_0,
            DataModel::OpenBadges3_0,
            DataModel::EuropeanLearningModelV3_3
        )]
        data_model: DataModel,
        #[values(HolderType::Individual, HolderType::Organization)] holder_type: HolderType,
    ) {
        use agent_library::template::aggregate::test_utils::{
            structured_schema, structured_schema_properties_attributes,
        };
        use oid4vc_core::claim_path_pointer::{check_claims_description_paths, ClaimPathSemantics};

        let template = Template {
            template_id: "t_appendix_b3".to_string(),
            data_model: data_model.clone(),
            holder_type: holder_type.clone(),
            schema: Box::new(Some(structured_schema())),
            schema_properties_attributes: Some(structured_schema_properties_attributes()),
            ..Default::default()
        };

        let config = credential_configuration_from_template(&template);
        let claims = config.credential_metadata.claims.unwrap_or_default();
        assert_eq!(
            config.format == "vc+sd-jwt",
            !claims.is_empty(),
            "{data_model:?}, {holder_type:?}"
        );
        assert_eq!(
            check_claims_description_paths(claims.iter().map(|claim| &claim.path), ClaimPathSemantics::Json),
            Ok(())
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
