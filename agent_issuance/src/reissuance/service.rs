use crate::application::{
    credential_offer::{ensure_offer_contains_credential, OfferCreation, PrepareOfferError},
    template_issuance::{
        published_template, template_credential_expiry, validate_credential_against_template, TemplateIssuanceError,
    },
};
use agent_library::state::LibraryState;
use agent_shared::handlers::{command_handler, query_handler, CommandHandlerError, QueryHandlerError};
use oid4vci::credential_format_profiles::CredentialFormats;
use shared_kernel::authorization::Caller;

use crate::{
    credential::{
        aggregate::{CredentialExpiry, Status},
        command::CredentialCommand,
        entity::Data,
        error::CredentialError,
    },
    reissuance::error::ReissuanceError,
    reissuance::{
        command::ReissuanceCommand,
        policy::{NoOpReissuancePolicy, ReissuancePolicy, ReissuancePolicyError, ReissuancePolicyRequest},
    },
    state::{IssuanceState, SERVER_CONFIG_ID},
};

pub struct CreateReissuanceRequest {
    pub reissuance_id: String,
    pub original_credential_id: String,
    pub new_credential_id: String,
    pub offer_id: String,
    pub credential_configuration_id: String,
    pub credential: serde_json::Value,
    pub expires_at: Option<CredentialExpiry>,

    pub reason: Option<String>,
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
}

#[derive(Debug)]
pub struct CreateReissuanceResponse {
    pub reissuance_id: String,
    pub original_credential_id: String,
    pub new_credential_id: String,
    pub offer_id: String,
    pub credential_configuration_id: String,
    pub credential_offer: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReissuanceServiceError {
    #[error("Original credential `{0}` was not found")]
    OriginalCredentialNotFound(String),

    #[error("Credential configuration `{0}` was not found")]
    CredentialConfigurationNotFound(String),

    #[error("Credential payload must be a JSON object")]
    InvalidCredentialPayload,

    #[error("Credential format is not supported for reissuance: {0}")]
    UnsupportedCredentialFormat(serde_json::Value),

    #[error(transparent)]
    Policy(#[from] ReissuancePolicyError),

    #[error(transparent)]
    Query(#[from] QueryHandlerError),
    #[error(transparent)]
    Credential(#[from] CommandHandlerError<CredentialError>),
    #[error(transparent)]
    Offer(#[from] PrepareOfferError),
    #[error(transparent)]
    Reissuance(#[from] CommandHandlerError<ReissuanceError>),
    #[error(transparent)]
    Template(#[from] TemplateIssuanceError),
    #[error("Only issued credentials can be reissued")]
    OriginalCredentialNotIssued,
    #[error("The requested configuration must match the original credential configuration")]
    CredentialConfigurationMismatch,
}

pub struct ReissuanceService<P = NoOpReissuancePolicy> {
    policy: P,
}

impl Default for ReissuanceService<NoOpReissuancePolicy> {
    fn default() -> Self {
        Self {
            policy: NoOpReissuancePolicy,
        }
    }
}

impl<P> ReissuanceService<P>
where
    P: ReissuancePolicy,
{
    pub fn new(policy: P) -> Self {
        Self { policy }
    }

    pub async fn create(
        &self,
        state: &IssuanceState,
        library_state: &LibraryState,
        caller: Caller,
        request: CreateReissuanceRequest,
    ) -> Result<CreateReissuanceResponse, ReissuanceServiceError> {
        let original_credential = query_handler(
            state.authorization_checker.clone(),
            caller.clone(),
            &request.original_credential_id,
            Some(&request.original_credential_id),
            &state.query.credential,
        )
        .await?
        .ok_or_else(|| ReissuanceServiceError::OriginalCredentialNotFound(request.original_credential_id.clone()))?;

        let (_, credential_configuration, authorization) = query_handler(
            state.authorization_checker.clone(),
            caller.clone(),
            SERVER_CONFIG_ID,
            Some(SERVER_CONFIG_ID),
            &state.query.server_config,
        )
        .await?
        .and_then(|server_config_view| {
            server_config_view
                .credential_configurations
                .get(&request.credential_configuration_id)
                .cloned()
        })
        .ok_or_else(|| {
            ReissuanceServiceError::CredentialConfigurationNotFound(request.credential_configuration_id.clone())
        })?;

        match &credential_configuration.credential_format {
            CredentialFormats::DcSdJwt(_) | CredentialFormats::VcSdJwt(_) => {}
            other => {
                return Err(ReissuanceServiceError::UnsupportedCredentialFormat(serde_json::json!(
                    other
                )));
            }
        }

        if !request.credential.is_object() {
            return Err(ReissuanceServiceError::InvalidCredentialPayload);
        }

        if original_credential.status != Status::Issued {
            return Err(ReissuanceServiceError::OriginalCredentialNotIssued);
        }
        if original_credential.credential_configuration.credential_format != credential_configuration.credential_format
        {
            return Err(ReissuanceServiceError::CredentialConfigurationMismatch);
        }
        let template = published_template(library_state, caller.clone(), &request.credential_configuration_id).await?;
        validate_credential_against_template(&request.credential, &template)?;
        let expires_at = template_credential_expiry(&template, request.expires_at)?;

        self.policy
            .authorize(&ReissuancePolicyRequest {
                original_credential_id: request.original_credential_id.clone(),
                credential_configuration_id: request.credential_configuration_id.clone(),
                triggered_by: request.triggered_by.clone(),
                trigger_type: request.trigger_type.clone(),
            })
            .await?;

        // Persist the audit relation before exposing a replacement through an offer.
        // These aggregates are not transactional: later failures may leave an incomplete relation.
        let create_reissuance_command = ReissuanceCommand::CreateReissuance {
            reissuance_id: request.reissuance_id.clone(),
            original_credential_id: original_credential.credential_id,
            new_credential_id: request.new_credential_id.clone(),
            offer_id: request.offer_id.clone(),
            credential_configuration_id: request.credential_configuration_id.clone(),
            reason: request.reason,
            trigger_type: request.trigger_type,
            triggered_by: request.triggered_by,
        };

        command_handler(
            state.authorization_checker.clone(),
            caller.clone(),
            &request.reissuance_id,
            &state.command.reissuance,
            create_reissuance_command,
        )
        .await?;

        let create_credential_command = CredentialCommand::CreateUnsignedCredential {
            credential_id: request.new_credential_id.clone(),
            data: Data {
                raw: request.credential,
            },
            credential_configuration: Box::new(credential_configuration),
            expires_at,
        };

        command_handler(
            state.authorization_checker.clone(),
            caller.clone(),
            &request.new_credential_id,
            &state.command.credential,
            create_credential_command,
        )
        .await?;

        ensure_offer_contains_credential(
            state,
            caller.clone(),
            &request.offer_id,
            &request.credential_configuration_id,
            authorization,
            &request.new_credential_id,
            OfferCreation::Fresh,
        )
        .await?;
        let credential_offer = query_handler(
            state.authorization_checker.clone(),
            caller,
            &request.offer_id,
            Some(&request.offer_id),
            &state.query.offer,
        )
        .await?
        .and_then(|offer| offer.form_url_encoded_credential_offer);

        Ok(CreateReissuanceResponse {
            reissuance_id: request.reissuance_id,
            original_credential_id: request.original_credential_id,
            new_credential_id: request.new_credential_id,
            offer_id: request.offer_id,
            credential_configuration_id: request.credential_configuration_id,
            credential_offer,
        })
    }
}

#[cfg(test)]
mod tests {
    use agent_issuance::credential::{aggregate::CredentialExpiry, command::CredentialCommand, entity::Data};
    use agent_issuance::reissuance::service::{CreateReissuanceRequest, ReissuanceService, ReissuanceServiceError};
    use agent_issuance::server_config::command::ServerConfigCommand;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::{initialize, issuance_state, IssuanceState, SERVER_CONFIG_ID};
    use agent_secret_manager::service::Service;
    use agent_shared::config::CredentialConfiguration;
    use agent_shared::handlers::{public_command_handler as command_handler, public_query_handler as query_handler};
    use agent_store::in_memory::InMemory;
    use serde_json::json;
    use shared_kernel::authorization::Caller;
    use std::sync::Arc;

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&state).await.unwrap();
        command_handler(
            SERVER_CONFIG_ID,
            &state.command.server_config,
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: serde_json::from_value(json!({
                    "credential_configuration_id": "001",
                    "format": "jwt_vc_json",
                    "credential_metadata": {"display": [{"name": "Unsupported format"}]},
                    "type": ["VerifiableCredential"]
                }))
                .unwrap(),
                provisioned: false,
            },
        )
        .await
        .unwrap();
        add_sd_jwt_credential_configuration(&state).await;
        state
    }

    async fn add_sd_jwt_credential_configuration(state: &IssuanceState) {
        let credential_configuration = serde_json::from_value::<CredentialConfiguration>(json!({
            "credential_configuration_id": "SD-JWT VC",
            "format": "dc+sd-jwt",
            "display": [
                {
                    "name": "SD-JWT VC Credential",
                    "locale": "en"
                }
            ],
            "claims": [
                {
                    "path": ["first_name"],
                    "display": [{ "name": "First Name", "locale": "en" }]
                },
                {
                    "path": ["last_name"],
                    "display": [{ "name": "Last Name", "locale": "en" }]
                },
                {
                    "path": ["dob"],
                    "display": [{ "name": "Date of Birth", "locale": "en" }]
                }
            ]
        }))
        .unwrap();

        command_handler(
            SERVER_CONFIG_ID,
            &state.command.server_config,
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration,
                provisioned: false,
            },
        )
        .await
        .unwrap();

        let credential_configuration = serde_json::from_value::<CredentialConfiguration>(json!({
            "credential_configuration_id": "VCDM SD-JWT VC",
            "format": "vc+sd-jwt",
            "type": ["VerifiableCredential"],
            "display": [
                {
                    "name": "VCDM SD-JWT Credential",
                    "locale": "en"
                }
            ],
            "claims": [
                {
                    "path": ["credentialSubject", "first_name"],
                    "display": [{ "name": "First Name", "locale": "en" }]
                },
                {
                    "path": ["credentialSubject", "last_name"],
                    "display": [{ "name": "Last Name", "locale": "en" }]
                },
                {
                    "path": ["credentialSubject", "dob"],
                    "display": [{ "name": "Date of Birth", "locale": "en" }]
                }
            ]
        }))
        .unwrap();

        command_handler(
            SERVER_CONFIG_ID,
            &state.command.server_config,
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration,
                provisioned: false,
            },
        )
        .await
        .unwrap();
    }

    async fn credential_configuration(
        state: &IssuanceState,
        credential_configuration_id: &str,
    ) -> oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject {
        query_handler(SERVER_CONFIG_ID, &state.query.server_config)
            .await
            .unwrap()
            .unwrap()
            .credential_configurations
            .get(credential_configuration_id)
            .unwrap()
            .1
            .clone()
    }

    async fn create_original_credential(state: &IssuanceState, credential_id: &str, credential_configuration_id: &str) {
        let credential_configuration = credential_configuration(state, credential_configuration_id).await;

        let credential = match credential_configuration_id {
            "SD-JWT VC" => json!({
                "first_name": "Ferris",
                "last_name": "Rustacean",
                "dob": "2010-01-01"
            }),
            "VCDM SD-JWT VC" => json!({
                "credentialSubject": {
                    "first_name": "Ferris",
                    "last_name": "Rustacean",
                    "dob": "2010-01-01"
                }
            }),
            _ => json!({
                "credentialSubject": {
                    "first_name": "Ferris",
                    "last_name": "Rustacean",
                    "dob": "2010-01-01"
                }
            }),
        };

        command_handler(
            credential_id,
            &state.command.credential,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: credential_id.to_string(),
                data: Data { raw: credential },
                credential_configuration: Box::new(credential_configuration),
                expires_at: CredentialExpiry::Never,
            },
        )
        .await
        .unwrap();
        command_handler(
            credential_id,
            &state.command.credential,
            CredentialCommand::SignCredential {
                credential_id: credential_id.to_owned(),
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

    async fn test_library_state() -> agent_library::state::LibraryState {
        let state = agent_library::state::library_state(&InMemory, &Default::default(), Default::default()).await;
        for id in ["SD-JWT VC", "VCDM SD-JWT VC"] {
            command_handler(
                id,
                &state.command.template,
                agent_library::template::command::TemplateCommand::CreateNewTemplate {
                    template_id: id.to_owned(),
                    source_template_id: None,
                    title: id.to_owned(),
                    display: Box::new(None),
                    data_model: Default::default(),
                    holder_type: Default::default(),
                    tags: None,
                    status: agent_library::template::aggregate::Status::Published,
                    visibility: Default::default(),
                    credential_expiration: Some(agent_library::template::aggregate::Expiration::Never),
                    description: None,
                    r#type: vec![],
                    schema: Box::new(None),
                    schema_properties_attributes: None,
                    holder_authorization: Default::default(),
                },
            )
            .await
            .unwrap();
        }
        state
    }

    fn reissuance_request(credential_configuration_id: &str, credential: serde_json::Value) -> CreateReissuanceRequest {
        CreateReissuanceRequest {
            reissuance_id: "reissuance-id".to_string(),
            original_credential_id: "11111111-1111-4111-8111-111111111111".to_string(),
            new_credential_id: "22222222-2222-4222-8222-222222222222".to_string(),
            offer_id: "offer-id".to_string(),
            credential_configuration_id: credential_configuration_id.to_string(),
            credential,
            expires_at: Some(CredentialExpiry::Never),
            reason: Some("data_changed".to_string()),
            trigger_type: Some("manual".to_string()),
            triggered_by: Some("unitrust".to_string()),
        }
    }

    #[async_std::test]
    async fn create_reissuance_rejects_missing_original_credential() {
        let state = test_state().await;
        let service = ReissuanceService::default();

        let error = service
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request(
                    "SD-JWT VC",
                    json!({
                        "first_name": "Ferris",
                        "last_name": "Reissued",
                        "dob": "2010-01-01"
                    }),
                ),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ReissuanceServiceError::OriginalCredentialNotFound(credential_id)
                if credential_id == "11111111-1111-4111-8111-111111111111"
        ));
    }

    #[async_std::test]
    async fn create_reissuance_rejects_non_object_payload() {
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "SD-JWT VC").await;
        let service = ReissuanceService::default();

        let error = service
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request("SD-JWT VC", json!("not-an-object")),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ReissuanceServiceError::InvalidCredentialPayload));
    }

    #[async_std::test]
    async fn create_reissuance_rejects_non_sd_jwt_configuration() {
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "001").await;
        let service = ReissuanceService::default();

        let error = service
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request(
                    "001",
                    json!({
                        "credentialSubject": {
                            "first_name": "Ferris",
                            "last_name": "Reissued",
                            "dob": "2010-01-01"
                        }
                    }),
                ),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ReissuanceServiceError::UnsupportedCredentialFormat(_)));
    }

    #[async_std::test]
    async fn create_reissuance_prepares_new_credential_offer_and_relation() {
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "SD-JWT VC").await;
        let service = ReissuanceService::default();

        let response = service
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request(
                    "SD-JWT VC",
                    json!({
                        "first_name": "Ferris",
                        "last_name": "Reissued",
                        "dob": "2010-01-01"
                    }),
                ),
            )
            .await
            .unwrap();

        assert_eq!(response.reissuance_id, "reissuance-id");
        assert_eq!(response.original_credential_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(response.new_credential_id, "22222222-2222-4222-8222-222222222222");
        assert_eq!(response.offer_id, "offer-id");
        assert_eq!(response.credential_configuration_id, "SD-JWT VC");

        let original_credential = query_handler("11111111-1111-4111-8111-111111111111", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let new_credential = query_handler("22222222-2222-4222-8222-222222222222", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let offer = query_handler("offer-id", &state.query.offer).await.unwrap().unwrap();
        let reissuance = query_handler("reissuance-id", &state.query.reissuance)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(original_credential.data.unwrap().raw["last_name"], json!("Rustacean"));
        assert_eq!(new_credential.data.unwrap().raw["last_name"], json!("Reissued"));
        assert_eq!(offer.credential_ids, vec!["22222222-2222-4222-8222-222222222222"]);
        assert_eq!(
            reissuance.original_credential_id,
            "11111111-1111-4111-8111-111111111111"
        );
        assert_eq!(reissuance.new_credential_id, "22222222-2222-4222-8222-222222222222");
        assert_eq!(reissuance.offer_id, "offer-id");
        assert_eq!(reissuance.credential_configuration_id, "SD-JWT VC");
        assert_eq!(reissuance.reason.as_deref(), Some("data_changed"));
        assert_eq!(reissuance.trigger_type.as_deref(), Some("manual"));
        assert_eq!(reissuance.triggered_by.as_deref(), Some("unitrust"));
    }

    #[async_std::test]
    async fn create_reissuance_prepares_vc_sd_jwt_credential_offer_and_relation() {
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "VCDM SD-JWT VC").await;
        let service = ReissuanceService::default();

        let response = service
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request(
                    "VCDM SD-JWT VC",
                    json!({
                        "credentialSubject": {
                            "first_name": "Ferris",
                            "last_name": "Reissued",
                            "dob": "2010-01-01"
                        }
                    }),
                ),
            )
            .await
            .unwrap();

        assert_eq!(response.reissuance_id, "reissuance-id");
        assert_eq!(response.original_credential_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(response.new_credential_id, "22222222-2222-4222-8222-222222222222");
        assert_eq!(response.offer_id, "offer-id");
        assert_eq!(response.credential_configuration_id, "VCDM SD-JWT VC");

        let original_credential = query_handler("11111111-1111-4111-8111-111111111111", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let new_credential = query_handler("22222222-2222-4222-8222-222222222222", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let offer = query_handler("offer-id", &state.query.offer).await.unwrap().unwrap();
        let reissuance = query_handler("reissuance-id", &state.query.reissuance)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            original_credential.data.unwrap().raw["credentialSubject"]["last_name"],
            json!("Rustacean")
        );
        assert_eq!(
            new_credential.data.unwrap().raw["credentialSubject"]["last_name"],
            json!("Reissued")
        );
        assert_eq!(offer.credential_ids, vec!["22222222-2222-4222-8222-222222222222"]);
        assert_eq!(
            reissuance.original_credential_id,
            "11111111-1111-4111-8111-111111111111"
        );
        assert_eq!(reissuance.new_credential_id, "22222222-2222-4222-8222-222222222222");
        assert_eq!(reissuance.offer_id, "offer-id");
        assert_eq!(reissuance.credential_configuration_id, "VCDM SD-JWT VC");
    }
    async fn assert_no_replacement(state: &IssuanceState) {
        assert!(
            query_handler("22222222-2222-4222-8222-222222222222", &state.query.credential)
                .await
                .unwrap()
                .is_none()
        );
        assert!(query_handler("offer-id", &state.query.offer).await.unwrap().is_none());
        assert!(query_handler("reissuance-id", &state.query.reissuance)
            .await
            .unwrap()
            .is_none());
    }

    #[async_std::test]
    async fn create_reissuance_rejects_pending_original() {
        let state = test_state().await;
        let configuration = credential_configuration(&state, "SD-JWT VC").await;
        command_handler(
            "11111111-1111-4111-8111-111111111111",
            &state.command.credential,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: "11111111-1111-4111-8111-111111111111".to_owned(),
                data: Data {
                    raw: json!({"first_name": "Ferris"}),
                },
                credential_configuration: Box::new(configuration),
                expires_at: CredentialExpiry::Never,
            },
        )
        .await
        .unwrap();
        let error = ReissuanceService::default()
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request("SD-JWT VC", json!({"first_name": "Ferris"})),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ReissuanceServiceError::OriginalCredentialNotIssued));
        assert_no_replacement(&state).await;
    }

    #[async_std::test]
    async fn create_reissuance_rejects_different_credential_type() {
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "VCDM SD-JWT VC").await;
        let error = ReissuanceService::default()
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request("SD-JWT VC", json!({"first_name": "Ferris"})),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ReissuanceServiceError::CredentialConfigurationMismatch));
        assert_no_replacement(&state).await;
    }

    #[async_std::test]
    async fn create_reissuance_enforces_template_state_schema_and_expiration() {
        use agent_issuance::application::template_issuance::TemplateIssuanceError;
        use agent_library::template::{
            aggregate::{Expiration, Status},
            command::TemplateCommand,
        };
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "SD-JWT VC").await;
        let library = test_library_state().await;
        let service = ReissuanceService::default();
        command_handler(
            "SD-JWT VC",
            &library.command.template,
            TemplateCommand::UpdateStatus {
                template_id: "SD-JWT VC".to_owned(),
                status: Status::Archived,
            },
        )
        .await
        .unwrap();
        let error = service
            .create(
                &state,
                &library,
                Caller::Anonymous,
                reissuance_request("SD-JWT VC", json!({"first_name": "Ferris"})),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ReissuanceServiceError::Template(TemplateIssuanceError::TemplateNotPublished)
        ));
        assert_no_replacement(&state).await;
        command_handler(
            "SD-JWT VC",
            &library.command.template,
            TemplateCommand::UpdateStatus {
                template_id: "SD-JWT VC".to_owned(),
                status: Status::Published,
            },
        )
        .await
        .unwrap();
        command_handler(
            "SD-JWT VC",
            &library.command.template,
            TemplateCommand::UpdateSchema {
                template_id: "SD-JWT VC".to_owned(),
                schema: json!({"type": "object", "required": ["dob"], "properties": {"dob": {"type": "string"}}}),
            },
        )
        .await
        .unwrap();
        let error = service
            .create(
                &state,
                &library,
                Caller::Anonymous,
                reissuance_request("SD-JWT VC", json!({"first_name": "Ferris"})),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ReissuanceServiceError::Template(TemplateIssuanceError::SchemaValidation { .. })
        ));
        assert_no_replacement(&state).await;
        command_handler(
            "SD-JWT VC",
            &library.command.template,
            TemplateCommand::UpdateCredentialExpiration {
                template_id: "SD-JWT VC".to_owned(),
                credential_expiration: Expiration::Duration("P1Y".to_owned()),
            },
        )
        .await
        .unwrap();
        let mut request = reissuance_request("SD-JWT VC", json!({"dob": "2010-01-01"}));
        request.expires_at = Some(CredentialExpiry::Fixed("2099-01-01T00:00:00Z".parse().unwrap()));
        let error = service
            .create(&state, &library, Caller::Anonymous, request)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ReissuanceServiceError::Template(TemplateIssuanceError::ExpirationExceedsTemplateLimit(_))
        ));
        assert_no_replacement(&state).await;
    }

    #[async_std::test]
    async fn failed_relation_cannot_leave_an_issuable_replacement() {
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "SD-JWT VC").await;
        command_handler(
            "reissuance-id",
            &state.command.reissuance,
            agent_issuance::reissuance::command::ReissuanceCommand::CreateReissuance {
                reissuance_id: "reissuance-id".to_owned(),
                original_credential_id: "original".to_owned(),
                new_credential_id: "other".to_owned(),
                offer_id: "other-offer".to_owned(),
                credential_configuration_id: "SD-JWT VC".to_owned(),
                reason: None,
                trigger_type: None,
                triggered_by: None,
            },
        )
        .await
        .unwrap();
        let error = ReissuanceService::default()
            .create(
                &state,
                &test_library_state().await,
                Caller::Anonymous,
                reissuance_request("SD-JWT VC", json!({"first_name": "Ferris"})),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ReissuanceServiceError::Reissuance(agent_shared::handlers::CommandHandlerError::Aggregate(
                cqrs_es::AggregateError::UserError(agent_issuance::reissuance::error::ReissuanceError::AlreadyExists)
            ))
        ));
        assert!(
            query_handler("22222222-2222-4222-8222-222222222222", &state.query.credential)
                .await
                .unwrap()
                .is_none()
        );
        assert!(query_handler("offer-id", &state.query.offer).await.unwrap().is_none());
    }

    #[async_std::test]
    async fn create_reissuance_defaults_expiration_from_template() {
        let state = test_state().await;
        create_original_credential(&state, "11111111-1111-4111-8111-111111111111", "SD-JWT VC").await;
        let library = test_library_state().await;
        command_handler(
            "SD-JWT VC",
            &library.command.template,
            agent_library::template::command::TemplateCommand::UpdateCredentialExpiration {
                template_id: "SD-JWT VC".to_owned(),
                credential_expiration: agent_library::template::aggregate::Expiration::DateTime(
                    "2099-01-01T00:00:00Z".to_owned(),
                ),
            },
        )
        .await
        .unwrap();
        let mut request = reissuance_request("SD-JWT VC", json!({"first_name": "Ferris"}));
        request.expires_at = None;
        let response = ReissuanceService::default()
            .create(&state, &library, Caller::Anonymous, request)
            .await
            .unwrap();
        let credential = query_handler(&response.new_credential_id, &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(credential.expires_at, Some("2099-01-01T00:00:00Z".parse().unwrap()));
    }
}
