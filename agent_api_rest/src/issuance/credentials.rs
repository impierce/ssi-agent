use crate::API_VERSION;
use agent_issuance::{
    credential::{aggregate::CredentialExpiry, command::CredentialCommand, entity::Data, views::CredentialView},
    offer::command::OfferCommand,
    server_config::queries::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hyper::header;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use tracing::info;
use utoipa::ToSchema;

#[derive(ToSchema)]
#[schema(as = CredentialView)]
pub struct CredentialViewSchema {
    pub foo: String,
    pub bar: i32,
}

/// Retrieve a credential
///
/// Retrieves an existing credential by its ID.
#[utoipa::path(
    get,
    path = "/credentials/{id}",
    tag = "Issuance",
    params(
        ("id" = String, Path, description = "Unique identifier of the Credential", example = "0001"),
    ),
    responses(
        (status = 200, description = "Credential found", body = [CredentialViewSchema])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential(State(state): State<IssuanceState>, Path(credential_id): Path<String>) -> Response {
    match query_handler(&credential_id, &state.query.credential).await {
        Ok(Some(credential_view)) => (StatusCode::OK, Json(credential_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsEndpointRequest {
    pub offer_id: String,
    pub credential: Value,
    #[serde(default)]
    pub is_signed: bool,
    pub credential_configuration_id: String,
    pub expires_at: CredentialExpiry,
}

/// Create a new credential
///
/// Create a new credential for a given subject.
#[utoipa::path(
    post,
    path = "/credentials",
    request_body(content = CredentialsEndpointRequest,
        examples(
            ("w3c-vc" = (summary = "W3C v1.1", description = "s0me descr1pti0n", value = json!({"offerId": "123", "credentialConfigurationId": "w3c_vc_credential", "credential": {"credentialSubject": {"first_name": "Ferris", "last_name": "Rustacean"}}}))),
            ("openbadges" = (summary = "Open Badges 3.0", description = "s0me descr1pti0n", external_value = "res/open-badge-request.json"))
        )
    ),
    tag = "Issuance",
    responses(
        (status = 201, description = "Successfully created a new credential.", body = CredentialViewSchema,
            headers(("Location" = String, description = "URL of the created resource")),
            examples(
                ("w3c-vc-1-1" = (summary = "W3C VC Data Model v1.1", description = "A credential following the W3C Verifiable Credentials Data Model v1.1", value = json!({"offerId": "0001"}))),
                ("openbadges-3-0" = (summary = "Open Badges 3.0", description = "An badge following the Open Badges Specification 3.0", value = json!({"foo": "bar"})))
            )
        ),
        (status = 400, description = "Invalid payload.")
    ),
)]
#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(state): State<IssuanceState>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    info!("Request Body: {}", payload);

    let Ok(CredentialsEndpointRequest {
        offer_id,
        credential: data,
        is_signed,
        credential_configuration_id,
        expires_at,
    }) = serde_json::from_value(payload)
    else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    if !(data.is_object() || data.is_string()) {
        return (StatusCode::BAD_REQUEST, "credential must be an object or a string").into_response();
    }

    let credential_id = uuid::Uuid::new_v4().to_string();

    let credential_configuration = Box::new(
        match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
            Ok(Some(ServerConfigView {
                credential_issuer_metadata:
                    Some(CredentialIssuerMetadata {
                        credential_configurations_supported,
                        ..
                    }),
                ..
            })) => {
                if let Some(credential_configuration) =
                    credential_configurations_supported.get(&credential_configuration_id)
                {
                    credential_configuration.clone()
                } else {
                    return (
                        StatusCode::NOT_FOUND,
                        format!("No Credential Configuration found with id: `{credential_configuration_id}`"),
                    )
                        .into_response();
                }
            }
            _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
    );

    let command = if is_signed {
        CredentialCommand::CreateSignedCredential {
            credential_id: credential_id.clone(),
            signed_credential: data,
        }
    } else {
        CredentialCommand::CreateUnsignedCredential {
            credential_id: credential_id.clone(),
            data: Data { raw: data },
            credential_configuration,
            expires_at,
        }
    };

    // Create an unsigned/signed credential.
    if command_handler(&credential_id, &state.command.credential, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Get the `CredentialIssuerMetadata` from the `ServerConfigView`.
    let credential_issuer_metadata = match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(ServerConfigView {
            credential_issuer_metadata: Some(credential_issuer_metadata),
            ..
        })) => Box::new(credential_issuer_metadata),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Create an offer if it does not exist yet.
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(_)) => {}
        _ => {
            if command_handler(
                &offer_id,
                &state.command.offer,
                OfferCommand::CreateCredentialOffer {
                    offer_id: offer_id.clone(),
                    credential_issuer_metadata,
                },
            )
            .await
            .is_err()
            {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    let command = OfferCommand::AddCredentials {
        offer_id: offer_id.clone(),
        credential_ids: vec![credential_id.clone()],
    };

    // Add the credential to the offer.
    if command_handler(&offer_id, &state.command.offer, command).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Return the credential.
    match query_handler(&credential_id, &state.query.credential).await {
        Ok(Some(CredentialView {
            data: Some(Data { raw }),
            ..
        })) => (
            StatusCode::CREATED,
            [(header::LOCATION, &format!("{API_VERSION}/credentials/{credential_id}"))],
            Json(raw),
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn all_credentials(State(state): State<IssuanceState>) -> Response {
    match query_handler("all_credentials", &state.query.all_credentials).await {
        Ok(Some(all_credentials_view)) => {
            let all_credentials = all_credentials_view.credentials.into_values().collect::<Vec<_>>();

            (StatusCode::OK, Json(all_credentials)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
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
                    .uri(&format!("{API_VERSION}/credentials"))
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
