use agent_issuance::{
    credential::{command::CredentialCommand, entity::Data, queries::CredentialView},
    handlers::{command_handler, query_handler},
    offer::command::OfferCommand,
    state::ApplicationState,
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hyper::header;
use serde_json::Value;
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn get_credentials(
    State(state): State<ApplicationState>,
    Path(credential_id): Path<String>,
) -> Response {
    // Get the credential if it exists.
    match query_handler(&credential_id, &state.query.credential).await {
        Ok(Some(CredentialView { data: Data { raw }, .. })) => (StatusCode::OK, Json(raw)).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn credentials(State(state): State<ApplicationState>, Json(payload): Json<Value>) -> Response {
    info!("Request Body: {}", payload);

    // TODO: should we rename this to `offer_id`?
    let subject_id = if let Some(subject_id) = payload["subjectId"].as_str() {
        subject_id
    } else {
        return (StatusCode::BAD_REQUEST, "subjectId is required").into_response();
    };

    let data = if payload["credential"].is_object() {
        payload["credential"].clone()
    } else {
        return (StatusCode::BAD_REQUEST, "credential is required").into_response();
    };

    let credential_id = uuid::Uuid::new_v4().to_string();

    let command = CredentialCommand::CreateUnsignedCredential {
        data: Data { raw: data },
        credential_format_template: serde_json::from_str(include_str!(
            "../../agent_issuance/res/credential_format_templates/openbadges_v3.json"
        ))
        .unwrap(),
    };

    // Create an unsigned credential.
    if command_handler(&credential_id, &state.command.credential, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Create an offer if it does not exist yet.
    match query_handler(subject_id, &state.query.offer).await {
        Ok(Some(_)) => {}
        _ => {
            if command_handler(subject_id, &state.command.offer, OfferCommand::CreateCredentialOffer)
                .await
                .is_err()
            {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    let command = OfferCommand::AddCredentials {
        credential_ids: vec![credential_id.clone()],
    };

    // Add the credential to the offer.
    if command_handler(subject_id, &state.command.offer, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Return the credential.
    match query_handler(&credential_id, &state.query.credential).await {
        Ok(Some(CredentialView { data: Data { raw }, .. })) => (
            StatusCode::CREATED,
            [(header::LOCATION, &format!("/v1/credentials/{credential_id}"))],
            Json(raw),
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        app,
        tests::{BASE_URL, SUBJECT_ID},
    };
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use lazy_static::lazy_static;
    use serde_json::json;
    use tower::Service;

    lazy_static! {
        pub static ref CREDENTIAL: serde_json::Value = json!({
            "@context": [
                "https://www.w3.org/2018/credentials/v1",
                "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
                ],
                "id": "http://example.com/credentials/3527",
                "type": ["VerifiableCredential", "OpenBadgeCredential"],
                "issuer": {
                    "id": "https://example.com/issuers/876543",
                    "type": "Profile",
                    "name": "Example Corp"
                },
            "issuanceDate": "2010-01-01T00:00:00Z",
            "name": "Teamwork Badge",
            "credentialSubject": {
                "first_name": "Ferris",
                "last_name": "Rustacean"
            }
        });
    }

    pub async fn credentials(app: &mut Router) {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/credentials")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "subjectId": SUBJECT_ID,
                            "credential": {
                                "credentialSubject": {
                                "first_name": "Ferris",
                                "last_name": "Rustacean"
                            }},
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

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
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "subjectId": SUBJECT_ID,
                            "credential": {
                                "credentialSubject": {
                                "first_name": "Ferris",
                                "last_name": "Rustacean"
                            }},
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, CREDENTIAL.clone());
    }

    #[tokio::test]
    async fn test_credentials_endpoint() {
        let state = in_memory::application_state().await;

        initialize(state.clone(), startup_commands(BASE_URL.clone())).await;

        let mut app = app(state);

        credentials(&mut app).await;
    }
}
