use super::offers::query_credential_issuer_metadata;
use crate::error::type_url;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_issuance::credential::aggregate::CredentialStatus;
use agent_issuance::{
    credential::{aggregate::CredentialExpiry, command::CredentialCommand, entity::Data},
    offer::command::OfferCommand,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::header;
use oauth_tsl::status_list::StatusType;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<IssuanceState>,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&credential_id, &state.query.credential)
        .await?
        .map(|credential_view| (StatusCode::OK, Json(credential_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsEndpointRequest {
    pub offer_id: String,
    pub credential: Value,
    #[serde(default)]
    pub is_signed: bool,
    pub credential_configuration_id: String,
    pub expires_at: CredentialExpiry,
}

#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(state): State<IssuanceState>,
    Json(CredentialsEndpointRequest {
        offer_id,
        credential,
        is_signed,
        credential_configuration_id,
        expires_at,
    }): Json<CredentialsEndpointRequest>,
) -> Result<Response, ApiError> {
    let credential_id = uuid::Uuid::new_v4().to_string();

    let command = if is_signed {
        // For a signed credential, ensure that the credential is a string.
        if !credential.is_string() {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Type")
                .type_url(type_url("issuance#invalid-credential-type"))
                .message("For signed credentials, the credential must be a string.")
                .finish());
        }

        CredentialCommand::CreateSignedCredential {
            credential_id: credential_id.clone(),
            signed_credential: credential,
        }
    } else {
        // For an unsigned credential, ensure that the credential is an object.
        if !credential.is_object() {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Type")
                .type_url(type_url("issuance#invalid-credential-type"))
                .message("For unsigned credentials, the credential must be an object.")
                .finish());
        }

        let credential_configuration = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
            .await?
            .and_then(|server_config_view| server_config_view.credential_issuer_metadata)
            .and_then(|credential_issuer_metadata| {
                credential_issuer_metadata
                    .credential_configurations_supported
                    .get(&credential_configuration_id)
                    .cloned()
            })
            .ok_or_else(|| {
                ApiError::builder(StatusCode::NOT_FOUND)
                    .title("No Credential Configuration Found")
                    .type_url(type_url("issuance#no-credential-configuration-found"))
                    .message(format!(
                        "No Credential Configuration found with id: `{credential_configuration_id}`"
                    ))
                    .finish()
            })?;

        CredentialCommand::CreateUnsignedCredential {
            credential_id: credential_id.clone(),
            data: Data { raw: credential },
            credential_configuration: Box::new(credential_configuration),
            expires_at,
        }
    };

    // Create an unsigned/signed credential.
    command_handler(&credential_id, &state.command.credential, command).await?;

    // Create an offer if it does not exist yet.
    if query_handler(&offer_id, &state.query.offer).await?.is_none() {
        let credential_issuer_metadata = query_credential_issuer_metadata(&state).await?;

        let command = OfferCommand::CreateCredentialOffer {
            offer_id: offer_id.clone(),
            credential_issuer_metadata: Box::new(credential_issuer_metadata),
        };

        command_handler(&offer_id, &state.command.offer, command).await?
    };

    let command = OfferCommand::AddCredentials {
        offer_id: offer_id.clone(),
        credential_ids: vec![credential_id.clone()],
    };

    // Add the credential to the offer.
    command_handler(&offer_id, &state.command.offer, command).await?;

    // Return the credential.
    query_handler(&credential_id, &state.query.credential)
        .await?
        .and_then(|credential_view| credential_view.data)
        .map(|data| {
            (
                StatusCode::CREATED,
                [(header::LOCATION, &format!("{API_VERSION}/credentials/{credential_id}"))],
                Json(data.raw),
            )
                .into_response()
        })
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[axum_macros::debug_handler]
pub(crate) async fn all_credentials(State(state): State<IssuanceState>) -> Result<Response, ApiError> {
    let all_credentials = query_handler("all_credentials", &state.query.all_credentials)
        .await?
        .map(|all_credentials_view| all_credentials_view.credentials.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_credentials)).into_response())
}

#[derive(Serialize, Deserialize)]
pub struct PatchCredentialEndpointRequest {
    pub status: StatusType,
}

pub const STATUSTYPESIZE: u8 = 2; // Amount of bits per status
pub const STATUSLISTSIZE: usize = 16384; // Amount of bytes in the status list. Equates to 65536 statuses.

/// Currently, this endpoint only supports patching the CredentialStatus of a credential according to the IETF OAuth Token Status List spec.
pub async fn patch_credential(
    State(state): State<IssuanceState>,
    Path(credential_id): Path<String>,
    Json(PatchCredentialEndpointRequest { status }): Json<PatchCredentialEndpointRequest>,
) -> Result<Response, ApiError> {
    if let Some(credential) = query_handler(&credential_id, &state.query.credential).await? {
        let credential_status: CredentialStatus;

        if credential.credential_status.is_some() {
            credential_status = CredentialStatus {
                index: credential.credential_status.as_ref().unwrap().index,
                status: status.clone(),
            }
        } else {
            let all_credentials = query_handler("all_credentials", &state.query.all_credentials)
                .await?
                .map(|all_credentials_view| all_credentials_view.credentials.into_values().collect::<Vec<_>>())
                .unwrap_or_default();

            let used_indices: Vec<usize> = all_credentials
                .iter()
                .filter_map(|c| c.credential_status.as_ref().map(|s| s.index))
                .collect();

            // Status Lists should only be filled up to 70%, the remaining 30% will be used for decoy/psuedo indices.
            // This greatly improves the privacy of the issuer.
            let statuses_per_byte: usize = 8 / STATUSTYPESIZE as usize;
            let status_list_number = used_indices.len() / ((STATUSLISTSIZE * statuses_per_byte) as f64 * 0.7) as usize;
            let mut rng = rand::rng();
            let mut random_index;

            loop {
                random_index = rng
                    .random_range((status_list_number * STATUSLISTSIZE)..((status_list_number + 1) * STATUSLISTSIZE));
                if !used_indices.contains(&random_index) {
                    break;
                }
            }

            credential_status = CredentialStatus {
                index: random_index,
                status,
            };
        }

        let command = CredentialCommand::SetCredentialStatus {
            credential_id: credential_id.clone(),
            credential_status,
        };

        command_handler(&credential_id, &state.command.credential, command).await?;
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::issuance::router;
    use crate::tests::{BASE_URL, CREDENTIAL_CONFIGURATION_ID, OFFER_ID};
    use crate::API_VERSION;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use lazy_static::lazy_static;
    use serde_json::json;
    use tower::Service as _;

    lazy_static! {
        pub static ref CREDENTIAL_SUBJECT: serde_json::Value = json!({
            "first_name": "Ferris",
            "last_name": "Rustacean"
        });
        pub static ref CREDENTIAL: serde_json::Value = json!({
            "@context": "https://www.w3.org/2018/credentials/v1",
            "type": [ "VerifiableCredential" ],
            "issuer": {
                "id": "https://my-domain.example.org/",
                "name": "UniCore"
            },
            "issuanceDate": "2010-01-01T00:00:00Z",
            "credentialSubject": CREDENTIAL_SUBJECT.clone()
        });
    }

    pub async fn credentials(app: &mut Router) {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/credentials"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": OFFER_ID,
                            "credential": {
                                "credentialSubject": CREDENTIAL_SUBJECT.clone()
                            },
                            "credentialConfigurationId": CREDENTIAL_CONFIGURATION_ID,
                            "expiresAt": "never"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let get_credentials_endpoint = response
            .headers()
            .get(http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, CREDENTIAL.clone());

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(get_credentials_endpoint)
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["raw"], CREDENTIAL.clone());
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_credentials_endpoint() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = router(issuance_state);
        credentials(&mut app).await;
    }
}
