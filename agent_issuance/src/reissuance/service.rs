use agent_shared::{
    config::config,
    handlers::{command_handler, query_handler},
    UrlAppendHelpers,
};
use oid4vci::{credential_format_profiles::CredentialFormats, credential_offer::GrantType};

use crate::{
    credential::{
        aggregate::{CredentialExpiry, CredentialRefreshService},
        command::CredentialCommand,
        entity::Data,
    },
    offer::command::OfferCommand,
    refresh_capability::service::{RefreshCapabilityService, RefreshCapabilityServiceError},
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
    pub expires_at: CredentialExpiry,

    pub reason: Option<String>,
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
    pub status_action: Option<String>,
}

#[derive(Debug)]
pub struct CreateReissuanceResponse {
    pub reissuance_id: String,
    pub original_credential_id: String,
    pub new_credential_id: String,
    pub offer_id: String,
    pub credential_configuration_id: String,
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
    #[error("Failed to query state: {0}")]
    Query(String),
    #[error("Failed to execute command: {0}")]
    Command(String),
    #[error(transparent)]
    RefreshCapability(#[from] RefreshCapabilityServiceError),
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
        request: CreateReissuanceRequest,
    ) -> Result<CreateReissuanceResponse, ReissuanceServiceError> {
        let original_credential = query_handler(&request.original_credential_id, &state.query.credential)
            .await
            .map_err(|err| ReissuanceServiceError::Query(err.to_string()))?
            .ok_or_else(|| {
                ReissuanceServiceError::OriginalCredentialNotFound(request.original_credential_id.clone())
            })?;

        let (_, credential_configuration, authorization, refresh_service) =
            query_handler(SERVER_CONFIG_ID, &state.query.server_config)
                .await
                .map_err(|err| ReissuanceServiceError::Query(err.to_string()))?
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

        self.policy
            .authorize(&ReissuancePolicyRequest {
                original_credential_id: request.original_credential_id.clone(),
                credential_configuration_id: request.credential_configuration_id.clone(),
                triggered_by: request.triggered_by.clone(),
                trigger_type: request.trigger_type.clone(),
            })
            .await?;

        let refresh_capability = RefreshCapabilityService::default()
            .create_for_credential(state, &request.new_credential_id, refresh_service.as_ref())
            .await?;

        let credential_refresh_service =
            refresh_service
                .as_ref()
                .zip(refresh_capability.as_ref())
                .map(|(refresh_service, refresh_capability)| CredentialRefreshService {
                    type_: refresh_service.type_.clone(),
                    url: config()
                        .public_url
                        .append_path_segment("credential-refresh")
                        .to_string(),
                    refresh_token: refresh_capability.refresh_reference.clone(),
                });

        let create_credential_command = CredentialCommand::CreateUnsignedCredential {
            credential_id: request.new_credential_id.clone(),
            data: Data {
                raw: request.credential,
            },
            credential_configuration: Box::new(credential_configuration.clone()),
            refresh_service: credential_refresh_service,
            expires_at: request.expires_at,
        };

        command_handler(
            &request.new_credential_id,
            &state.command.credential,
            create_credential_command,
        )
        .await
        .map_err(|err| ReissuanceServiceError::Command(err.to_string()))?;

        if query_handler(&request.offer_id, &state.query.offer)
            .await
            .map_err(|err| ReissuanceServiceError::Query(err.to_string()))?
            .is_none()
        {
            let tx_code_constraints = authorization
                .pre_authorized
                .then_some(authorization.tx_code_constraints)
                .flatten();

            let grant_types = vec![if authorization.pre_authorized {
                GrantType::PreAuthorizedCode
            } else {
                GrantType::AuthorizationCode
            }];

            let create_offer_command = OfferCommand::CreateCredentialOffer {
                offer_id: request.offer_id.clone(),
                credential_configuration_ids: vec![request.credential_configuration_id.clone()],
                grant_types,
                tx_code_constraints,
                delivery_options: None,
            };

            command_handler(&request.offer_id, &state.command.offer, create_offer_command)
                .await
                .map_err(|err| ReissuanceServiceError::Command(err.to_string()))?;
        }

        let add_credentials_command = OfferCommand::AddCredentials {
            offer_id: request.offer_id.clone(),
            credential_ids: vec![request.new_credential_id.clone()],
            credential_configuration_ids: vec![request.credential_configuration_id.clone()],
        };

        command_handler(&request.offer_id, &state.command.offer, add_credentials_command)
            .await
            .map_err(|err| ReissuanceServiceError::Command(err.to_string()))?;

        let create_reissuance_command = ReissuanceCommand::CreateReissuance {
            reissuance_id: request.reissuance_id.clone(),
            original_credential_id: original_credential.credential_id,
            new_credential_id: request.new_credential_id.clone(),
            offer_id: request.offer_id.clone(),
            credential_configuration_id: request.credential_configuration_id.clone(),
            reason: request.reason,
            trigger_type: request.trigger_type,
            triggered_by: request.triggered_by,
            status_action: request.status_action,
        };

        command_handler(
            &request.reissuance_id,
            &state.command.reissuance,
            create_reissuance_command,
        )
        .await
        .map_err(|err| ReissuanceServiceError::Command(err.to_string()))?;

        Ok(CreateReissuanceResponse {
            reissuance_id: request.reissuance_id,
            original_credential_id: request.original_credential_id,
            new_credential_id: request.new_credential_id,
            offer_id: request.offer_id,
            credential_configuration_id: request.credential_configuration_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use agent_issuance::credential::{aggregate::CredentialExpiry, command::CredentialCommand, entity::Data};
    use agent_issuance::refresh_capability::aggregate::RefreshCapabilityStatus;
    use agent_issuance::reissuance::service::{CreateReissuanceRequest, ReissuanceService, ReissuanceServiceError};
    use agent_issuance::server_config::command::ServerConfigCommand;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::{initialize, IssuanceState, SERVER_CONFIG_ID};
    use agent_secret_manager::service::Service;
    use agent_shared::config::CredentialConfiguration;
    use agent_shared::handlers::{command_handler, query_handler};
    use agent_store::{in_memory::InMemory, issuance_state};
    use serde_json::json;
    use std::sync::Arc;

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();
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

    async fn add_refreshable_sd_jwt_credential_configuration(state: &IssuanceState) {
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
            ],
            "refreshService": {
                "type": "VerifiableCredentialRefreshService2021"
            }
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
                refresh_service: None,
                expires_at: CredentialExpiry::Never,
            },
        )
        .await
        .unwrap();
    }

    fn reissuance_request(credential_configuration_id: &str, credential: serde_json::Value) -> CreateReissuanceRequest {
        CreateReissuanceRequest {
            reissuance_id: "reissuance-id".to_string(),
            original_credential_id: "original-credential-id".to_string(),
            new_credential_id: "new-credential-id".to_string(),
            offer_id: "offer-id".to_string(),
            credential_configuration_id: credential_configuration_id.to_string(),
            credential,
            expires_at: CredentialExpiry::Never,
            reason: Some("data_changed".to_string()),
            trigger_type: Some("manual".to_string()),
            triggered_by: Some("unitrust".to_string()),
            status_action: None,
        }
    }

    #[async_std::test]
    async fn create_reissuance_rejects_missing_original_credential() {
        let state = test_state().await;
        let service = ReissuanceService::default();

        let error = service
            .create(
                &state,
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
                if credential_id == "original-credential-id"
        ));
    }

    #[async_std::test]
    async fn create_reissuance_rejects_non_object_payload() {
        let state = test_state().await;
        create_original_credential(&state, "original-credential-id", "SD-JWT VC").await;
        let service = ReissuanceService::default();

        let error = service
            .create(&state, reissuance_request("SD-JWT VC", json!("not-an-object")))
            .await
            .unwrap_err();

        assert!(matches!(error, ReissuanceServiceError::InvalidCredentialPayload));
    }

    #[async_std::test]
    async fn create_reissuance_rejects_non_sd_jwt_configuration() {
        let state = test_state().await;
        create_original_credential(&state, "original-credential-id", "001").await;
        let service = ReissuanceService::default();

        let error = service
            .create(
                &state,
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
        create_original_credential(&state, "original-credential-id", "SD-JWT VC").await;
        let service = ReissuanceService::default();

        let response = service
            .create(
                &state,
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
        assert_eq!(response.original_credential_id, "original-credential-id");
        assert_eq!(response.new_credential_id, "new-credential-id");
        assert_eq!(response.offer_id, "offer-id");
        assert_eq!(response.credential_configuration_id, "SD-JWT VC");

        let original_credential = query_handler("original-credential-id", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let new_credential = query_handler("new-credential-id", &state.query.credential)
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
        assert_eq!(offer.credential_ids, vec!["new-credential-id"]);
        assert_eq!(reissuance.original_credential_id, "original-credential-id");
        assert_eq!(reissuance.new_credential_id, "new-credential-id");
        assert_eq!(reissuance.offer_id, "offer-id");
        assert_eq!(reissuance.credential_configuration_id, "SD-JWT VC");
        assert_eq!(reissuance.reason.as_deref(), Some("data_changed"));
        assert_eq!(reissuance.trigger_type.as_deref(), Some("manual"));
        assert_eq!(reissuance.triggered_by.as_deref(), Some("unitrust"));
        assert_eq!(reissuance.status_action, None);
    }

    #[async_std::test]
    async fn create_reissuance_creates_refresh_capability_for_refreshable_configuration() {
        let state = test_state().await;
        add_refreshable_sd_jwt_credential_configuration(&state).await;
        create_original_credential(&state, "original-credential-id", "SD-JWT VC").await;
        let service = ReissuanceService::default();

        service
            .create(
                &state,
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

        // wrong spelling because this is generated by a `format!("all_{}s")`
        let all_refresh_capabilities = query_handler("all_refresh_capabilitys", &state.query.all_refresh_capabilities)
            .await
            .unwrap()
            .unwrap();

        let refresh_capability = all_refresh_capabilities
            .refresh_capabilities
            .values()
            .find(|refresh_capability| refresh_capability.credential_id == "new-credential-id")
            .expect("new credential should have a refresh capability");

        assert_eq!(refresh_capability.status, RefreshCapabilityStatus::Active);

        let new_credential = query_handler("new-credential-id", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let new_credential_data = new_credential.data.unwrap().raw;

        assert_eq!(
            new_credential_data["refreshService"],
            json!({
                "type": "VerifiableCredentialRefreshService2021",
                "url": "https://my-domain.example.org/credential-refresh",
                "refreshToken": refresh_capability.refresh_reference
            })
        );
    }

    #[async_std::test]
    async fn create_reissuance_prepares_vc_sd_jwt_credential_offer_and_relation() {
        let state = test_state().await;
        create_original_credential(&state, "original-credential-id", "VCDM SD-JWT VC").await;
        let service = ReissuanceService::default();

        let response = service
            .create(
                &state,
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
        assert_eq!(response.original_credential_id, "original-credential-id");
        assert_eq!(response.new_credential_id, "new-credential-id");
        assert_eq!(response.offer_id, "offer-id");
        assert_eq!(response.credential_configuration_id, "VCDM SD-JWT VC");

        let original_credential = query_handler("original-credential-id", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let new_credential = query_handler("new-credential-id", &state.query.credential)
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
        assert_eq!(offer.credential_ids, vec!["new-credential-id"]);
        assert_eq!(reissuance.original_credential_id, "original-credential-id");
        assert_eq!(reissuance.new_credential_id, "new-credential-id");
        assert_eq!(reissuance.offer_id, "offer-id");
        assert_eq!(reissuance.credential_configuration_id, "VCDM SD-JWT VC");
    }
}
