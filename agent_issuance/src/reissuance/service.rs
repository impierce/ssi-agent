use crate::{
    application::{
        credential_offer::create_credential_offer_command,
        template_issuance::{published_template, template_credential_expiry, validate_credential_against_template},
    },
    credential::{
        aggregate::{CredentialExpiry, Status},
        command::CredentialCommand,
        entity::Data,
    },
    offer::command::OfferCommand,
    reissuance::{aggregate::Reissuance, command::ReissuanceCommand, error::ReissuanceError},
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_library::state::LibraryState;
use agent_shared::handlers::{command_handler, query_handler, CommandHandlerError, QueryHandlerError};
use oid4vci::credential_format_profiles::CredentialFormats;
use shared_kernel::authorization::Caller;

/// What a replacement credential consists of.
#[derive(Debug, Clone)]
pub struct Replacement {
    /// The configuration the replacement is issued with. It must issue the same format as the original's.
    pub credential_configuration_id: String,
    /// The replacement's content, a JSON object that fits the configuration's published template.
    pub credential: serde_json::Value,
    /// `None` takes the template's default expiry.
    pub expires_at: Option<CredentialExpiry>,
    pub trigger: ReissuanceTrigger,
}

/// What started a reissuance and why. Free-form, and recorded on the relation exactly as given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReissuanceTrigger {
    pub reason: Option<String>,
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
}

/// A prepared reissuance: the recorded relation, and the offer the holder redeems for the replacement.
#[derive(Debug)]
pub struct PreparedReissuance {
    pub relation: Reissuance,
    /// The form-url-encoded credential offer, as for any other offer.
    pub credential_offer: Option<String>,
}

/// Prepares a replacement for an issued credential: a new credential, a new offer for it, and the relation from
/// the original to the replacement. The original and its status are left as they are. Identifiers are generated.
///
/// Every check and the configured [`ReissuancePolicy`](crate::reissuance::policy::ReissuancePolicy) run before
/// anything is written. Then the relation is written first, the credential second and the offer last, so a failed
/// relation never leaves an issuable replacement. The writes are not transactional: a failure after the relation
/// leaves a relation without a usable offer.
pub async fn reissue_credential(
    state: &IssuanceState,
    library_state: &LibraryState,
    caller: Caller,
    original_credential_id: &str,
    replacement: Replacement,
) -> Result<PreparedReissuance, ReissuanceError> {
    let authorization_checker = state.authorization_checker.clone();

    let original = query_handler(
        authorization_checker.clone(),
        caller.clone(),
        original_credential_id,
        Some(original_credential_id),
        &state.query.credential,
    )
    .await
    .map_err(query_failed)?
    .ok_or_else(|| ReissuanceError::OriginalCredentialNotFound(original_credential_id.to_owned()))?;

    let configuration_id = &replacement.credential_configuration_id;
    let (_, credential_configuration, authorization) = query_handler(
        authorization_checker.clone(),
        caller.clone(),
        SERVER_CONFIG_ID,
        Some(SERVER_CONFIG_ID),
        &state.query.server_config,
    )
    .await
    .map_err(query_failed)?
    .and_then(|server_config| server_config.credential_configurations.get(configuration_id).cloned())
    .ok_or_else(|| ReissuanceError::CredentialConfigurationNotFound(configuration_id.clone()))?;

    if !matches!(
        credential_configuration.credential_format,
        CredentialFormats::DcSdJwt(_) | CredentialFormats::VcSdJwt(_)
    ) {
        return Err(ReissuanceError::UnsupportedCredentialFormat(serde_json::json!(
            credential_configuration.credential_format
        )));
    }
    if !replacement.credential.is_object() {
        return Err(ReissuanceError::InvalidCredentialPayload);
    }
    if original.status != Status::Issued {
        return Err(ReissuanceError::OriginalCredentialNotIssued);
    }
    if original.credential_configuration.credential_format != credential_configuration.credential_format {
        return Err(ReissuanceError::CredentialConfigurationMismatch);
    }

    let template = published_template(library_state, caller.clone(), configuration_id)
        .await
        .map_err(ReissuanceError::Template)?;
    validate_credential_against_template(&replacement.credential, &template).map_err(ReissuanceError::Template)?;
    let expires_at =
        template_credential_expiry(&template, replacement.expires_at.clone()).map_err(ReissuanceError::Template)?;

    state
        .ports
        .reissuance_policy
        .check(&original, &replacement)
        .await
        .map_err(ReissuanceError::NotAllowed)?;

    let reissuance_id = uuid::Uuid::new_v4().to_string();
    let new_credential_id = uuid::Uuid::new_v4().to_string();
    let offer_id = uuid::Uuid::new_v4().to_string();
    let Replacement {
        credential_configuration_id,
        credential,
        expires_at: _,
        trigger: ReissuanceTrigger {
            reason,
            trigger_type,
            triggered_by,
        },
    } = replacement;

    command_handler(
        authorization_checker.clone(),
        caller.clone(),
        &reissuance_id,
        &state.command.reissuance,
        ReissuanceCommand::CreateReissuance {
            reissuance_id: reissuance_id.clone(),
            original_credential_id: original.credential_id,
            new_credential_id: new_credential_id.clone(),
            offer_id: offer_id.clone(),
            credential_configuration_id: credential_configuration_id.clone(),
            reason,
            trigger_type,
            triggered_by,
        },
    )
    .await
    .map_err(|error| command_failed(error, |error| error))?;

    command_handler(
        authorization_checker.clone(),
        caller.clone(),
        &new_credential_id,
        &state.command.credential,
        CredentialCommand::CreateUnsignedCredential {
            credential_id: new_credential_id.clone(),
            data: Data { raw: credential },
            credential_configuration: Box::new(credential_configuration),
            expires_at,
        },
    )
    .await
    .map_err(|error| command_failed(error, ReissuanceError::ReplacementRejected))?;

    for command in [
        create_credential_offer_command(&offer_id, &credential_configuration_id, &authorization, None),
        OfferCommand::AddCredentials {
            offer_id: offer_id.clone(),
            credential_ids: vec![new_credential_id],
            template_ids: vec![credential_configuration_id],
        },
    ] {
        command_handler(
            authorization_checker.clone(),
            caller.clone(),
            &offer_id,
            &state.command.offer,
            command,
        )
        .await
        .map_err(|error| command_failed(error, |error| ReissuanceError::Failed(error.to_string())))?;
    }

    let relation = query_handler(
        authorization_checker.clone(),
        caller.clone(),
        &reissuance_id,
        Some(&reissuance_id),
        &state.query.reissuance,
    )
    .await
    .map_err(query_failed)?
    .ok_or_else(|| ReissuanceError::Failed(format!("relation `{reissuance_id}` is missing after it was recorded")))?;
    let credential_offer = query_handler(
        authorization_checker,
        caller,
        &offer_id,
        Some(&offer_id),
        &state.query.offer,
    )
    .await
    .map_err(query_failed)?
    .and_then(|offer| offer.form_url_encoded_credential_offer);

    Ok(PreparedReissuance {
        relation,
        credential_offer,
    })
}

fn query_failed(error: QueryHandlerError) -> ReissuanceError {
    match error {
        QueryHandlerError::Authorization(error) => ReissuanceError::Authorization(error),
        QueryHandlerError::Persistence(error) => ReissuanceError::Persistence(error),
    }
}

fn command_failed<E: std::error::Error>(
    error: CommandHandlerError<E>,
    rejected: impl FnOnce(E) -> ReissuanceError,
) -> ReissuanceError {
    match error {
        CommandHandlerError::Authorization(error) => ReissuanceError::Authorization(error),
        CommandHandlerError::Aggregate(error) => ReissuanceError::from_aggregate(error, rejected),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::template_issuance::TemplateIssuanceError,
        credential::aggregate::Credential,
        reissuance::policy::{ReissuancePolicy, ReissuancePolicyError},
        server_config::command::ServerConfigCommand,
        services::IssuanceServices,
        state::{initialize, issuance_state},
    };
    use agent_library::template::{
        aggregate::{Expiration, Status as TemplateStatus},
        command::TemplateCommand,
    };
    use agent_secret_manager::service::Service;
    use agent_shared::config::CredentialConfiguration;
    use agent_shared::handlers::{public_command_handler, public_query_handler};
    use agent_store::in_memory::InMemory;
    use serde_json::{json, Value};
    use shared_kernel::authorization::{
        AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest,
    };
    use std::sync::Arc;

    const ORIGINAL: &str = "11111111-1111-4111-8111-111111111111";
    const DC_SD_JWT: &str = "SD-JWT VC";
    const VC_SD_JWT: &str = "VCDM SD-JWT VC";
    const JWT_VC_JSON: &str = "001";

    async fn add_configuration(state: &IssuanceState, configuration: Value) {
        public_command_handler(
            SERVER_CONFIG_ID,
            &state.command.server_config,
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: serde_json::from_value::<CredentialConfiguration>(configuration).unwrap(),
                provisioned: false,
            },
        )
        .await
        .unwrap();
    }

    async fn test_state() -> IssuanceState {
        let state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
        initialize(&state).await.unwrap();
        add_configuration(
            &state,
            json!({
                "credential_configuration_id": JWT_VC_JSON,
                "format": "jwt_vc_json",
                "type": ["VerifiableCredential"]
            }),
        )
        .await;
        add_configuration(
            &state,
            json!({"credential_configuration_id": DC_SD_JWT, "format": "dc+sd-jwt"}),
        )
        .await;
        add_configuration(
            &state,
            json!({
                "credential_configuration_id": VC_SD_JWT,
                "format": "vc+sd-jwt",
                "type": ["VerifiableCredential"]
            }),
        )
        .await;
        state
    }

    async fn test_library_state() -> LibraryState {
        let state = agent_library::state::library_state(&InMemory, &Default::default(), Default::default()).await;
        for template_id in [DC_SD_JWT, VC_SD_JWT] {
            public_command_handler(
                template_id,
                &state.command.template,
                TemplateCommand::CreateNewTemplate {
                    template_id: template_id.to_owned(),
                    source_template_id: None,
                    title: template_id.to_owned(),
                    display: Box::new(None),
                    data_model: Default::default(),
                    holder_type: Default::default(),
                    tags: None,
                    status: TemplateStatus::Published,
                    visibility: Default::default(),
                    credential_expiration: Some(Expiration::Never),
                    description: None,
                    r#type: vec![],
                    schema: Box::new(None),
                    schema_properties_attributes: None,
                    holder_authorization: Default::default(),
                    origin: None,
                },
            )
            .await
            .unwrap();
        }
        state
    }

    async fn update_template(library_state: &LibraryState, command: TemplateCommand) {
        public_command_handler(DC_SD_JWT, &library_state.command.template, command)
            .await
            .unwrap();
    }

    async fn create_credential(state: &IssuanceState, credential_configuration_id: &str, raw: Value) {
        let credential_configuration = public_query_handler(SERVER_CONFIG_ID, &state.query.server_config)
            .await
            .unwrap()
            .unwrap()
            .credential_configurations[credential_configuration_id]
            .1
            .clone();
        public_command_handler(
            ORIGINAL,
            &state.command.credential,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: ORIGINAL.to_owned(),
                data: Data { raw },
                credential_configuration: Box::new(credential_configuration),
                expires_at: CredentialExpiry::Never,
            },
        )
        .await
        .unwrap();
    }

    async fn issue_original(state: &IssuanceState, credential_configuration_id: &str, raw: Value) {
        create_credential(state, credential_configuration_id, raw).await;
        public_command_handler(
            ORIGINAL,
            &state.command.credential,
            CredentialCommand::SignCredential {
                credential_id: ORIGINAL.to_owned(),
                subject_id: None,
                overwrite: false,
                proofs: None,
                status_list_id: "status-list".to_owned(),
                index: 0,
            },
        )
        .await
        .unwrap();
    }

    fn replacement(credential_configuration_id: &str, credential: Value) -> Replacement {
        Replacement {
            credential_configuration_id: credential_configuration_id.to_owned(),
            credential,
            expires_at: Some(CredentialExpiry::Never),
            trigger: ReissuanceTrigger {
                reason: Some("data_changed".to_owned()),
                trigger_type: Some("manual".to_owned()),
                triggered_by: Some("unitrust".to_owned()),
            },
        }
    }

    async fn reissue(
        state: &IssuanceState,
        library_state: &LibraryState,
        replacement: Replacement,
    ) -> Result<PreparedReissuance, ReissuanceError> {
        reissue_credential(state, library_state, Caller::Anonymous, ORIGINAL, replacement).await
    }

    async fn credentials_and_offers(state: &IssuanceState) -> (usize, usize) {
        let credentials = public_query_handler("all_credentials", &state.query.all_credentials)
            .await
            .unwrap()
            .map_or(0, |view| view.credentials.len());
        let offers = public_query_handler("all_offers", &state.query.all_offers)
            .await
            .unwrap()
            .map_or(0, |view| view.offers.len());
        (credentials, offers)
    }

    #[tokio::test]
    async fn rejects_a_missing_original_credential() {
        let state = test_state().await;

        let error = reissue(&state, &test_library_state().await, replacement(DC_SD_JWT, json!({})))
            .await
            .unwrap_err();

        assert!(matches!(error, ReissuanceError::OriginalCredentialNotFound(id) if id == ORIGINAL));
    }

    #[tokio::test]
    async fn rejects_an_original_that_is_still_pending() {
        let state = test_state().await;
        create_credential(&state, DC_SD_JWT, json!({"first_name": "Ferris"})).await;

        let error = reissue(&state, &test_library_state().await, replacement(DC_SD_JWT, json!({})))
            .await
            .unwrap_err();

        assert!(matches!(error, ReissuanceError::OriginalCredentialNotIssued));
    }

    #[tokio::test]
    async fn rejects_a_payload_that_is_not_a_json_object() {
        let state = test_state().await;
        issue_original(&state, DC_SD_JWT, json!({"first_name": "Ferris"})).await;

        let error = reissue(
            &state,
            &test_library_state().await,
            replacement(DC_SD_JWT, json!("text")),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, ReissuanceError::InvalidCredentialPayload));
    }

    #[tokio::test]
    async fn rejects_a_configuration_whose_format_cannot_be_reissued() {
        let state = test_state().await;
        issue_original(
            &state,
            JWT_VC_JSON,
            json!({"credentialSubject": {"first_name": "Ferris"}}),
        )
        .await;

        let error = reissue(
            &state,
            &test_library_state().await,
            replacement(JWT_VC_JSON, json!({"credentialSubject": {}})),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, ReissuanceError::UnsupportedCredentialFormat(_)));
    }

    #[tokio::test]
    async fn rejects_a_configuration_of_a_different_credential_type() {
        let state = test_state().await;
        issue_original(
            &state,
            VC_SD_JWT,
            json!({"credentialSubject": {"first_name": "Ferris"}}),
        )
        .await;

        let error = reissue(&state, &test_library_state().await, replacement(DC_SD_JWT, json!({})))
            .await
            .unwrap_err();

        assert!(matches!(error, ReissuanceError::CredentialConfigurationMismatch));
    }

    #[tokio::test]
    async fn enforces_the_template_state_schema_and_expiration() {
        let state = test_state().await;
        issue_original(&state, DC_SD_JWT, json!({"first_name": "Ferris"})).await;
        let library_state = test_library_state().await;
        update_template(
            &library_state,
            TemplateCommand::UpdateCredentialExpiration {
                template_id: DC_SD_JWT.to_owned(),
                credential_expiration: Expiration::Duration("P1Y".to_owned()),
            },
        )
        .await;
        let mut too_late = replacement(DC_SD_JWT, json!({"first_name": "Ferris"}));
        too_late.expires_at = Some(CredentialExpiry::Fixed("2099-01-01T00:00:00Z".parse().unwrap()));

        let error = reissue(&state, &library_state, too_late).await.unwrap_err();

        assert!(matches!(
            error,
            ReissuanceError::Template(TemplateIssuanceError::ExpirationExceedsTemplateLimit(_))
        ));
        assert_eq!(credentials_and_offers(&state).await, (1, 0));
    }

    #[tokio::test]
    async fn defaults_the_expiration_from_the_template() {
        let state = test_state().await;
        issue_original(&state, DC_SD_JWT, json!({"first_name": "Ferris"})).await;
        let library_state = test_library_state().await;
        update_template(
            &library_state,
            TemplateCommand::UpdateCredentialExpiration {
                template_id: DC_SD_JWT.to_owned(),
                credential_expiration: Expiration::DateTime("2099-01-01T00:00:00Z".to_owned()),
            },
        )
        .await;
        let mut without_expiry = replacement(DC_SD_JWT, json!({"first_name": "Ferris"}));
        without_expiry.expires_at = None;

        let prepared = reissue(&state, &library_state, without_expiry).await.unwrap();

        let replacement = public_query_handler(&prepared.relation.new_credential_id, &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(replacement.expires_at, Some("2099-01-01T00:00:00Z".parse().unwrap()));
    }

    #[tokio::test]
    async fn prepares_an_offer_and_relation_for_dc_sd_jwt() {
        let state = test_state().await;
        issue_original(
            &state,
            DC_SD_JWT,
            json!({"first_name": "Ferris", "last_name": "Rustacean"}),
        )
        .await;

        let prepared = reissue(
            &state,
            &test_library_state().await,
            replacement(DC_SD_JWT, json!({"first_name": "Ferris", "last_name": "Reissued"})),
        )
        .await
        .unwrap();

        let relation = prepared.relation;
        let replacement = public_query_handler(&relation.new_credential_id, &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let offer = public_query_handler(&relation.offer_id, &state.query.offer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                relation.original_credential_id.as_str(),
                relation.credential_configuration_id.as_str()
            ),
            (ORIGINAL, DC_SD_JWT)
        );
        assert_eq!(
            (relation.reason, relation.trigger_type, relation.triggered_by),
            (
                Some("data_changed".to_owned()),
                Some("manual".to_owned()),
                Some("unitrust".to_owned())
            )
        );
        assert_eq!(replacement.data.unwrap().raw["last_name"], json!("Reissued"));
        assert_eq!(offer.credential_ids, vec![relation.new_credential_id]);
        assert_eq!(prepared.credential_offer, offer.form_url_encoded_credential_offer);
    }

    #[tokio::test]
    async fn prepares_an_offer_and_relation_for_vc_sd_jwt() {
        let state = test_state().await;
        issue_original(
            &state,
            VC_SD_JWT,
            json!({"credentialSubject": {"last_name": "Rustacean"}}),
        )
        .await;

        let prepared = reissue(
            &state,
            &test_library_state().await,
            replacement(VC_SD_JWT, json!({"credentialSubject": {"last_name": "Reissued"}})),
        )
        .await
        .unwrap();

        let replacement = public_query_handler(&prepared.relation.new_credential_id, &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            replacement.data.unwrap().raw["credentialSubject"]["last_name"],
            json!("Reissued")
        );
        assert!(prepared.credential_offer.is_some());
    }

    /// Denies recording reissuance relations, and allows everything else.
    struct DenyRelations;

    #[async_trait::async_trait]
    impl AuthorizationChecker for DenyRelations {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            match request.operation {
                AuthorizationOperation::Command {
                    operation_name: "issuance.credential_reissuances.create",
                    ..
                } => Err(AuthorizationError::Forbidden),
                _ => Ok(()),
            }
        }
    }

    #[tokio::test]
    async fn a_failed_relation_never_leaves_an_issuable_replacement() {
        let mut state = test_state().await;
        issue_original(&state, DC_SD_JWT, json!({"first_name": "Ferris"})).await;
        state.authorization_checker = Arc::new(DenyRelations);

        let error = reissue(
            &state,
            &test_library_state().await,
            replacement(DC_SD_JWT, json!({"first_name": "Ferris"})),
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            ReissuanceError::Authorization(AuthorizationError::Forbidden)
        ));
        assert_eq!(credentials_and_offers(&state).await, (1, 0));
    }

    struct DenyEveryReissuance;

    #[async_trait::async_trait]
    impl ReissuancePolicy for DenyEveryReissuance {
        async fn check(&self, _: &Credential, _: &Replacement) -> Result<(), ReissuancePolicyError> {
            Err(ReissuancePolicyError::NotAllowed)
        }
    }

    #[tokio::test]
    async fn the_configured_policy_can_deny_a_reissuance() {
        let mut state = test_state().await;
        issue_original(&state, DC_SD_JWT, json!({"first_name": "Ferris"})).await;
        state.ports.reissuance_policy = Arc::new(DenyEveryReissuance);

        let error = reissue(
            &state,
            &test_library_state().await,
            replacement(DC_SD_JWT, json!({"first_name": "Ferris"})),
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            ReissuanceError::NotAllowed(ReissuancePolicyError::NotAllowed)
        ));
        assert_eq!(credentials_and_offers(&state).await, (1, 0));
    }
}
