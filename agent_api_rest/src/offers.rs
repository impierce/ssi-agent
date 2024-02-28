use agent_issuance::{
    handlers::{command_handler, query_handler},
    offer::command::OfferCommand,
    server_config::queries::ServerConfigView,
    state::{ApplicationState, SERVER_CONFIG_ID},
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::Value;
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<ApplicationState>, Json(payload): Json<Value>) -> impl IntoResponse {
    info!("offers endpoint");
    info!("Received request: {:?}", payload);

    let subject_id = if let Some(subject_id) = payload["subjectId"].as_str() {
        subject_id
    } else {
        info!("Returning 400");
        return (StatusCode::BAD_REQUEST, "subjectId is required".to_string()).into_response();
    };

    // Get the `CredentialIssuerMetadata` from the `ServerConfigView`.
    let credential_issuer_metadata = match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(ServerConfigView {
            credential_issuer_metadata: Some(credential_issuer_metadata),
            ..
        })) => credential_issuer_metadata,
        _ => {
            info!("Returning 500");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match command_handler(
        subject_id,
        &state.command.offer,
        OfferCommand::CreateFormUrlEncodedCredentialOffer {
            credential_issuer_metadata,
        },
    )
    .await
    {
        Ok(_) => {}
        _ => {
            info!("Returning 500");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match query_handler(subject_id, &state.query.offer).await {
        Ok(Some(offer_view)) => (StatusCode::OK, Json(offer_view.form_url_encoded_credential_offer)).into_response(),
        _ => {
            info!("Returning 500");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use crate::{
        app,
        credentials::tests::credentials,
        tests::{BASE_URL, SUBJECT_ID},
    };

    use super::*;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::{
        credential_format_profiles::CredentialFormats,
        credential_offer::{CredentialOffer, CredentialOfferQuery, Grants, PreAuthorizedCode},
    };
    use serde_json::json;
    use tower::Service;

    pub async fn offers(app: &mut Router) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/offers")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "subjectId": SUBJECT_ID
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let value: Value = serde_json::from_slice(&body).unwrap();
        let CredentialOfferQuery::CredentialOffer(CredentialOffer {
            grants:
                Some(Grants {
                    pre_authorized_code:
                        Some(PreAuthorizedCode {
                            pre_authorized_code, ..
                        }),
                    ..
                }),
            ..
        }) = CredentialOfferQuery::<CredentialFormats>::from_str(value.as_str().unwrap()).unwrap()
        else {
            unreachable!()
        };

        pre_authorized_code
    }

    #[tokio::test]
    async fn test_offers_endpoint() {
        let state = in_memory::application_state().await;

        initialize(state.clone(), startup_commands(BASE_URL.clone())).await;

        let mut app = app(state);

        credentials(&mut app).await;
        let _pre_authorized_code = offers(&mut app).await;
    }
}
