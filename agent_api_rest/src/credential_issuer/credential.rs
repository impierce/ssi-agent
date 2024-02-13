use agent_issuance::{
    handlers::{command_handler, query_handler},
    offer::command::OfferCommand,
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_auth::AuthBearer;
use oid4vci::credential_request::CredentialRequest;

#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<ApplicationState>,
    AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
) -> impl IntoResponse {
    println!("access_token: {:#?}", access_token);
    let offer_id = state
        .offer
        .load_access_token(&access_token)
        .await
        .unwrap()
        .unwrap()
        .offer_id;
    println!("offer_id: {:#?}", offer_id);

    let credential_id = state.offer.load(&offer_id).await.unwrap().unwrap().credential_id;
    println!("credential_id: {:#?}", credential_id);

    let credential = state.credential.load(&credential_id).await.unwrap().unwrap().credential;

    println!("HERE");

    let view = state.server_config.load("SERVCONFIG-0001").await.unwrap().unwrap();
    let (credential_issuer_metadata, authorization_server_metadata) = (
        view.credential_issuer_metadata.unwrap(),
        view.authorization_server_metadata.unwrap(),
    );

    println!("HERE");

    let command = OfferCommand::CreateCredentialResponse {
        credential_issuer_metadata,
        authorization_server_metadata,
        credential_request,
        credential,
    };
    println!("HERE");

    match command_handler(offer_id.clone(), &state.offer, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    println!("HERE");

    match query_handler(offer_id, &state.offer).await {
        Ok(Some(view)) => {
            dbg!(&view);
            (StatusCode::OK, Json(view.credential_response.unwrap())).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
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
    use agent_issuance::{startup_commands::startup_commands_server_config, state::initialize};
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

        initialize(state.clone(), startup_commands_server_config(BASE_URL.clone())).await;

        let mut app = app(state);

        let _response = credentials(&mut app).await.unwrap();
        let pre_authorized_code = offers(&mut app).await;

        let access_token = token(&mut app, pre_authorized_code).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/openid4vci/credential"))
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
