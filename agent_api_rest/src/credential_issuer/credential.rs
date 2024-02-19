use agent_issuance::{
    credential::queries::CredentialView,
    handlers::{command_handler, query_handler},
    offer::{
        command::OfferCommand,
        queries::{AccessTokenView, OfferView},
    },
    server_config::queries::ServerConfigView,
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_auth::AuthBearer;
use core::panic;
use oid4vci::credential_request::CredentialRequest;
use tracing::info;

use crate::SERVER_CONFIG_ID;

#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<ApplicationState>,
    AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
    // TODO: implement official oid4vci error response. This TODO is also in the `token` endpoint.
) -> impl IntoResponse {
    info!("credential endpoint");
    info!("Access Token: {:?}", access_token);
    info!("Received request: {:?}", credential_request);

    // Use the `access_token` to get the `offer_id` from the `AccessTokenView`.
    let offer_id = match query_handler(&access_token, &state.query.access_token).await {
        Ok(Some(AccessTokenView { offer_id })) => offer_id,
        _ => {
            info!("Returning 401");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Use the `offer_id` to get the `credential_ids` from the `OfferView`.
    let credential_ids = match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(OfferView { credential_ids, .. })) => credential_ids,
        _ => {
            info!("Returning 500");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Use the `credential_ids` to get the `credentials` from the `CredentialView`.
    let mut credentials = vec![];
    for credential_id in credential_ids {
        let credential = match query_handler(&credential_id, &state.query.credential).await {
            Ok(Some(CredentialView { data, .. })) => data,
            _ => {
                info!("Returning 500");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        credentials.push(credential);
    }

    // Get the `credential_issuer_metadata` and `authorization_server_metadata` from the `ServerConfigView`.
    let (credential_issuer_metadata, authorization_server_metadata) =
        match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
            Ok(Some(ServerConfigView {
                credential_issuer_metadata: Some(credential_issuer_metadata),
                authorization_server_metadata,
            })) => (credential_issuer_metadata, Box::new(authorization_server_metadata)),
            _ => {
                info!("Returning 500");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

    let command = OfferCommand::CreateCredentialResponse {
        credential_issuer_metadata,
        authorization_server_metadata,
        credential_request,
        credentials,
    };

    // Use the `offer_id` to create a `CredentialResponse` from the `CredentialRequest` and `credentials`.
    match command_handler(&offer_id, &state.command.offer, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        _ => {
            info!("Returning 500");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Use the `offer_id` to get the `credential_response` from the `OfferView`.
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(OfferView {
            credential_response: Some(credential_response),
            ..
        })) => (StatusCode::OK, Json(credential_response)).into_response(),
        _ => {
            info!("Returning 500");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        app, credential_issuer::token::tests::token, credentials::tests::credentials, offers::tests::offers,
        tests::BASE_URL,
    };

    use super::*;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_credential_endpoint() {
        let state = in_memory::application_state().await;

        initialize(state.clone(), startup_commands(BASE_URL.clone())).await;

        let mut app = app(state);

        credentials(&mut app).await;
        let pre_authorized_code = offers(&mut app).await;

        let access_token = token(&mut app, pre_authorized_code).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {}", access_token))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "format": "jwt_vc_json",
                            "credential_definition": {
                                "type": [
                                    "VerifiableCredential",
                                    "OpenBadgeCredential"
                                ]
                            },
                            "proof": {
                                "proof_type": "jwt",
                                "jwt": "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lkIjoiZGlkOmtleT\
                                p6Nk1rdWlSS3ExZktyekFYZVNOaUd3cnBKUFB1Z1k4QXhKWUE1Y3BDdlpDWUJEN0IjejZNa3VpUktxMWZLcnpB\
                                WGVTTmlHd3JwSlBQdWdZOEF4SllBNWNwQ3ZaQ1lCRDdCIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWt1aVJLcTFmS3\
                                J6QVhlU05pR3dycEpQUHVnWThBeEpZQTVjcEN2WkNZQkQ3QiIsImF1ZCI6Imh0dHA6Ly8xOTIuMTY4LjEuMTI3\
                                OjMwMzMvIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoidW5zYWZlX2Nfbm9uY2\
                                UifQ.wR2e4VUnVjG6IK9cntcqvc_8KEJQUd3SEjsPZwDYDlYqijZ4ZaQLxyHtzNmLkIS3FpChLrZrcvIUJrZxr\
                                WcKAg"
                            }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            json!({
                    "format": "jwt_vc_json",
                    "credential": "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa3F5WmpEZmhzeVo1YzZOdUp\
                    oYm9zV2tTajg2Mmp5V2lDQ0tIRHpOTkttOGtoI3o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5Lb\
                    ThraCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCIsIn\
                    N1YiI6ImRpZDprZXk6ejZNa3VpUktxMWZLcnpBWGVTTmlHd3JwSlBQdWdZOEF4SllBNWNwQ3ZaQ1lCRDdCIiwiZXhwIjo5OTk5\
                    OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIi\
                    wiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3AwL2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6\
                    Ly9leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZmlhYmxlQ3JlZGVudGlhbCIsIk9wZW5CYWRnZU\
                    NyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rcXlaakRmaHN5WjVjNk51Smhib3NXa1NqODYyanlXaUNDS0hEek5O\
                    S204a2giLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5hbWUiOiJUZWFtd29yayBCYWRnZSIsImNyZW\
                    RlbnRpYWxTdWJqZWN0Ijp7ImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4iLCJpZCI6ImRpZDpr\
                    ZXk6ejZNa3VpUktxMWZLcnpBWGVTTmlHd3JwSlBQdWdZOEF4SllBNWNwQ3ZaQ1lCRDdCIn19fQ.Sesb2jqkBF0usFzvKrXrdbh\
                    Akq2zbeSfrJFh6Wvza3Y8nL5n9Ep_pL5PIM0F0HlSM3s6mrMH36RScqm-lA1oDg"
                }
            )
        );
    }
}
