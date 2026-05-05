use crate::error::type_url;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_issuance::status_list::command::StatusListCommand;
use agent_issuance::{
    credential::{
        aggregate::{Credential, CredentialExpiry, CredentialStatus},
        command::CredentialCommand,
        entity::Data,
    },
    offer::command::OfferCommand,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{
    map_open_badges_input_to_credential, DataModel, Status as TemplateStatus, Template,
};
use axum::Extension;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::header;
use oauth_tsl::status_list::StatusType;
use oid4vci::credential_offer::GrantType;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Get credential by ID
///
/// Retrieves a credential by its ID.
#[utoipa::path(
    get,
    path = "/credentials/{credential_id}",
    operation_id = "get_credential_by_id",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Successfully retrieved credential", body = Credential)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<Arc<IssuanceState>>,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&credential_id, &state.query.credential)
        .await?
        .map(|credential_view| (StatusCode::OK, Json(credential_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsEndpointRequest {
    pub template_id: String,
    pub offer_id: String,
    pub credential: Value,
    #[serde(default)]
    pub is_signed: bool,
    pub expires_at: CredentialExpiry,
}

/// Create a credential
///
/// Creates a verifiable credential based on the provided template and data. An offer is created for the provided offer ID.
#[utoipa::path(
    post,
    path = "/credentials",
    operation_id = "create_credential",
    tags = ["Credentials", "Issuance"],
    responses(
        (status = 201, description = "Credential created successfully",
            headers(("Location" = String, description = "URI of the newly created credential"))
        )
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(issuance_state): State<Arc<IssuanceState>>,
    Extension(library_state): Extension<Arc<LibraryState>>,
    Json(CredentialsEndpointRequest {
        template_id,
        offer_id,
        credential,
        is_signed,
        expires_at,
    }): Json<CredentialsEndpointRequest>,
) -> Result<Response, ApiError> {
    let credential_id = uuid::Uuid::new_v4().to_string();

    // The credential configuration ID is derived from the template ID.
    let credential_configuration_id = template_id.clone();

    // Validate that template_id is not empty.
    if template_id.is_empty() {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Missing Template ID")
            .type_url(type_url("issuance#missing-template-id"))
            .message("The `templateId` field is required and must not be empty.")
            .finish());
    }

    // Look up the template by ID.
    let template: Template = query_handler(&template_id, &library_state.query.template)
        .await
        .map_err(|_| {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Template Query Error")
                .type_url(type_url("issuance#template-query-error"))
                .message(format!(
                    "An error occurred while looking up the template with id: `{template_id}`"
                ))
                .finish()
        })?
        .filter(|t| t.status != TemplateStatus::Deleted)
        .ok_or_else(|| {
            ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .type_url(type_url("issuance#template-not-found"))
                .message(format!("No template found with id: `{template_id}`"))
                .finish()
        })?;

    // If the template has a schema, validate the credential against it.
    // Only validate unsigned credentials (objects) - signed credentials are pre-built JWTs
    // and cannot be validated against a template schema.
    if !is_signed {
        if let Some(schema) = template.schema.as_ref() {
            // For OpenBadges 3.0 templates, the schema uses flat dot-notation keys (e.g. "achievement.name").
            // The API consumer sends data wrapped in "credentialSubject", so we unwrap it before validation.
            let data_to_validate = if template.data_model == Some(DataModel::OpenBadges3_0) {
                credential.get("credentialSubject").unwrap_or(&credential)
            } else {
                &credential
            };
            validate_credential_against_schema(data_to_validate, schema).map_err(|e| *e)?;
        }
    }

    // For OpenBadges 3.0 templates, map the flat input (conforming to the template schema)
    // to the nested OBv3 credential structure expected by the issuance pipeline.
    let credential = if !is_signed && template.data_model == Some(DataModel::OpenBadges3_0) {
        // Extract the credentialSubject content (flat dot-notation keys) for mapping.
        let flat_input = credential.get("credentialSubject").unwrap_or(&credential);
        map_open_badges_input_to_credential(flat_input)
    } else {
        credential
    };

    let (_, credential_configuration, authorization) =
        query_handler(SERVER_CONFIG_ID, &issuance_state.query.server_config)
            .await?
            .and_then(|server_config_view| {
                server_config_view
                    .credential_configurations
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

        CredentialCommand::CreateUnsignedCredential {
            credential_id: credential_id.clone(),
            data: Data { raw: credential },
            credential_configuration: Box::new(credential_configuration.clone()),
            expires_at,
        }
    };

    // Create an unsigned/signed credential.
    command_handler(&credential_id, &issuance_state.command.credential, command).await?;

    // Create an offer if it does not exist yet.
    if query_handler(&offer_id, &issuance_state.query.offer).await?.is_none() {
        // Extract the tx_code_constraints from the credential configuration if available.
        let tx_code_constraints = authorization
            .pre_authorized
            .then_some(authorization.tx_code_constraints)
            .flatten();

        let grant_types = vec![if authorization.pre_authorized {
            GrantType::PreAuthorizedCode
        } else {
            GrantType::AuthorizationCode
        }];

        let command = OfferCommand::CreateCredentialOffer {
            offer_id: offer_id.clone(),
            credential_configuration_ids: vec![credential_configuration_id.clone()],
            grant_types,
            tx_code_constraints,
            delivery_options: None,
        };

        command_handler(&offer_id, &issuance_state.command.offer, command).await?
    };

    let command = OfferCommand::AddCredentials {
        offer_id: offer_id.clone(),
        credential_ids: vec![credential_id.clone()],
        credential_configuration_ids: vec![credential_configuration_id],
    };

    // Add the credential to the offer.
    command_handler(&offer_id, &issuance_state.command.offer, command).await?;

    // Return the credential.
    query_handler(&credential_id, &issuance_state.query.credential)
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

/// List all credentials
///
/// Lists all credentials including their current status and metadata.
#[utoipa::path(
    get,
    path = "/credentials",
    operation_id = "get_all_credentials",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "List of all credentials", body = [Credential])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn all_credentials(State(issuance_state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let all_credentials = query_handler("all_credentials", &issuance_state.query.all_credentials)
        .await?
        .map(|all_credentials_view| all_credentials_view.credentials.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_credentials)).into_response())
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PatchCredentialEndpointRequest {
    pub credential_status: StatusType,
}

/// Currently, this endpoint only supports patching the CredentialStatus of a credential according to the IETF OAuth Token Status List spec.
pub async fn patch_credential(
    State(state): State<Arc<IssuanceState>>,
    Path(credential_id): Path<String>,
    Json(PatchCredentialEndpointRequest {
        credential_status: status,
    }): Json<PatchCredentialEndpointRequest>,
) -> Result<Response, ApiError> {
    if let Some(credential) = query_handler(&credential_id, &state.query.credential).await? {
        let credential_status = CredentialStatus {
            index: credential.credential_status.index,
            status,
            status_list_url: credential.credential_status.status_list_url.clone(),
        };

        let command = CredentialCommand::UpdateCredentialStatus {
            credential_id: credential_id.clone(),
            credential_status: credential_status.clone(),
        };

        command_handler(&credential_id, &state.command.credential, command).await?;

        let command = StatusListCommand::UpdateIndex {
            index: credential_status.index,
            status,
        };

        let status_list_url = credential_status.status_list_url.clone();
        let status_list_id = status_list_url
            .split('/')
            .next_back()
            .ok_or(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?; // This is an Internal Server Error because if this line fails that means we stored an incorect URL in our own credential.

        command_handler(status_list_id, &state.command.status_list, command).await?;

        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND))
    }
}

/// Validates the credential data against the template's JSON Schema.
///
/// Returns a detailed error response if validation fails, listing all schema violations.
fn validate_credential_against_schema(credential: &Value, schema: &Value) -> Result<(), Box<ApiError>> {
    let validator = jsonschema::validator_for(schema).map_err(|e| {
        Box::new(
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Invalid Template Schema")
                .type_url(type_url("issuance#invalid-template-schema"))
                .message(format!("The template's schema is not a valid JSON Schema: {e}"))
                .finish(),
        )
    })?;

    let errors: Vec<String> = validator
        .iter_errors(credential)
        .map(|e| format!("Path `{}`: {} (schema path: {})", e.instance_path(), e, e.schema_path()))
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(Box::new(
            ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Credential Schema Validation Failed")
                .type_url(type_url("issuance#credential-schema-validation-failed"))
                .message(format!(
                    "The credential does not match the template schema. Violations:\n{}",
                    errors
                        .iter()
                        .enumerate()
                        .map(|(i, e)| format!("  [{}] {}", i + 1, e))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
                .finish(),
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::tests::{OFFER_ID, TEMPLATE_ID};
    use crate::v0::issuance::{credential_issuer::token_status_list::tests::create_test_signed_credential, router};
    use crate::API_VERSION;
    use agent_issuance::{
        server_config::command::ServerConfigCommand, services::IssuanceServices, state::initialize,
        state::SERVER_CONFIG_ID,
    };
    use agent_library::template::aggregate::Status;
    use agent_library::template::command::TemplateCommand;
    use agent_library::template::event::Visibility;
    use agent_secret_manager::service::Service;
    use agent_secret_manager::subject::Subject;
    use agent_shared::config::{CredentialConfiguration, TESTINDEX};
    use agent_store::in_memory::InMemory;
    use agent_store::{issuance_state, library_state};
    use axum::{
        body::{self, Body},
        http::{self, Request, StatusCode},
        Router,
    };
    use lazy_static::lazy_static;
    use oauth_tsl::relying_party::check_status_in_status_list_token_jwt;
    use oauth_tsl::relying_party::{decompress_gzip, StatusListTokenResponseType};
    use serde_json::json;
    use tower::Service as _;

    use jsonwebtoken::{decode_header, Algorithm, DecodingKey};
    use oid4vc_core::authentication::verify::Verify;

    lazy_static! {
        pub static ref CREDENTIAL_SUBJECT: serde_json::Value = json!({
            "first_name": "Ferris",
            "last_name": "Rustacean"
        });

        // The credentialStatus id/uri only contains a relative path, since we only need to have the correct route for them in the tests.
        // This test credential is tested after creation but before signing, therefore it misses a few last fields which are set during signing.
        // Please look at the comments in agent_issuance/src/credential/aggregate.rs `SignCredential` for more information.
        pub static ref VC_DM_1_1_CREDENTIAL: serde_json::Value = json!({
            "@context": [ "https://www.w3.org/2018/credentials/v1" ],
            "type": [ "VerifiableCredential" ],
            "name": "Verifiable Credential",
            "issuer": {
                "name": "UniCore"
            },
            "credentialSubject": CREDENTIAL_SUBJECT.clone(),
        });
    }

    /// Creates a test template in the library state and registers a matching credential
    /// configuration in the issuance state. Returns the template ID.
    /// The `pre_authorized` parameter controls the authorization flow type for the credential configuration.
    pub async fn create_test_template_with_auth(
        library_state: &LibraryState,
        issuance_state: &IssuanceState,
        pre_authorized: bool,
    ) -> String {
        let template_id = TEMPLATE_ID.to_string();

        let command = TemplateCommand::CreateTemplate {
            template_id: template_id.clone(),
            source_template_id: None,
            title: "Test Template".to_string(),
            display: Box::new(None),
            data_model: agent_library::template::aggregate::DataModel::W3CVcDataModelV1_1,
            creator: None,
            holder_type: agent_library::template::aggregate::HolderType::Individual,
            tags: None,
            status: Status::Published,
            visibility: Visibility::Private,
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(Some(json!({
                "type": "object",
                "properties": {
                    "credentialSubject": {
                        "type": "object",
                        "properties": {
                            "first_name": { "type": "string" },
                            "last_name": { "type": "string" }
                        },
                        "required": ["first_name", "last_name"]
                    }
                },
                "required": ["credentialSubject"]
            }))),
            schema_properties_attributes: None,
        };

        agent_shared::handlers::command_handler(&template_id, &library_state.command.template, command)
            .await
            .unwrap();

        // Register a credential configuration matching the template_id.
        // This simulates what the credential_configuration_projection does in production.
        let credential_configuration: CredentialConfiguration = serde_json::from_value(json!({
            "credential_configuration_id": template_id,
            "format": "jwt_vc_json",
            "type": ["VerifiableCredential"],
            "display": [{ "name": "Verifiable Credential", "locale": "en" }],
            "authorization": { "pre_authorized": pre_authorized }
        }))
        .unwrap();

        let command = ServerConfigCommand::UpdateCredentialConfiguration {
            credential_configuration,
            provisioned: false,
        };

        agent_shared::handlers::command_handler(SERVER_CONFIG_ID, &issuance_state.command.server_config, command)
            .await
            .unwrap();

        template_id
    }

    /// Creates a test template with pre-authorized credential configuration (default).
    pub async fn create_test_template(library_state: &LibraryState, issuance_state: &IssuanceState) -> String {
        create_test_template_with_auth(library_state, issuance_state, true).await
    }

    /// This function creates and tests a credential and returns the endpoint where this credential can be accessed.
    pub async fn credentials(app: &mut Router) -> String {
        credentials_with_template(app, TEMPLATE_ID).await
    }

    /// This function creates and tests a credential with a specific template ID and returns the endpoint where this credential can be accessed.
    pub async fn credentials_with_template(app: &mut Router, template_id: &str) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/credentials"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "templateId": template_id,
                            "offerId": OFFER_ID,
                            "credential": {
                                "credentialSubject": CREDENTIAL_SUBJECT.clone(),
                            },
                            "expiresAt": "never"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        println!("Response: {:?}", response);

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
        assert_eq!(body, VC_DM_1_1_CREDENTIAL.clone());

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(get_credentials_endpoint.clone())
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
        assert_eq!(body["data"]["raw"], VC_DM_1_1_CREDENTIAL.clone());

        get_credentials_endpoint
    }

    pub async fn patch_credential(app: &mut Router, credential_endpoint: String) {
        let patch_response = app
            .call(
                Request::builder()
                    .method(http::Method::PATCH)
                    .uri(&credential_endpoint)
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credentialStatus": "INVALID"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

        // Fetch the Status List Token to check the updated status
        let token_status_list_response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/ietf-oauth-token-status-list/0")
                    .header(http::header::ACCEPT, StatusListTokenResponseType::Jwt.to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body_bytes = body::to_bytes(token_status_list_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let jwt_status_list_token = decompress_gzip(&body_bytes).unwrap();
        let jwt_header = decode_header(&jwt_status_list_token).unwrap();

        let key_id = jwt_header.kid.unwrap();
        let relying_party_state = Subject::test_subject().await;
        let public_key = relying_party_state.public_key(&key_id).await.unwrap();
        let decoding_key = match jwt_header.alg {
            Algorithm::EdDSA => DecodingKey::from_ed_der(&public_key),
            Algorithm::ES256 => DecodingKey::from_ec_der(&public_key),
            _ => {
                panic!("Unsupported algorithm: {:?}", jwt_header.alg);
            }
        };

        let status = check_status_in_status_list_token_jwt(&jwt_status_list_token, TESTINDEX, decoding_key).unwrap();

        assert_eq!(status, StatusType::INVALID as u8);
    }

    #[tokio::test]
    async fn test_patch_credential() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_test_template(&library_state, &issuance_state).await;

        let mut app = router((issuance_state.clone(), library_state));

        let credential_endpoint = create_test_signed_credential(&mut app, &issuance_state).await;
        patch_credential(&mut app, credential_endpoint).await;
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_credentials_endpoint() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_test_template(&library_state, &issuance_state).await;

        let mut app = router((issuance_state.clone(), library_state));

        credentials(&mut app).await;
    }
}
