use std::sync::Arc;

use agent_issuance::{
    state::IssuanceState, status_list::application::token_status_list_service::TokenStatusListService,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use http::StatusCode;
use hyper::header;
use oauth_tsl::relying_party::StatusListTokenResponseType;

use crate::v0::issuance::error::PublicError;

pub async fn token_status_list(
    State(state): State<Arc<IssuanceState>>,
    Path(status_list_id): Path<String>,
) -> Result<Response, PublicError> {
    let token_status_list_service = TokenStatusListService {};

    let compressed_jwt_token = token_status_list_service
        .create_gzip_status_list_jwt_token(status_list_id, &state)
        .await?;

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                StatusListTokenResponseType::Jwt.to_string().as_str(),
            ),
            (header::CONTENT_ENCODING, "gzip"),
        ],
        compressed_jwt_token,
    )
        .into_response())
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;

    use agent_authorization::services::AuthorizationServices;
    use agent_issuance::{
        services::IssuanceServices,
        state::{initialize, IssuanceState},
    };
    use agent_secret_manager::{service::Service, subject::Subject};
    use agent_shared::config::{config, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT};
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state, library_state};
    use axum::{
        body::{self, Body},
        Router,
    };
    use http::{Request, StatusCode};
    use jsonwebtoken::{decode_header, Algorithm, DecodingKey};
    use oauth_tsl::{
        relying_party::{decompress_gzip, decrypt_status_list_token, StatusListTokenResponseType},
        tokens::status_list_token::StatusListTyp,
    };
    use oid4vc_core::authentication::verify::Verify;

    use crate::v0::{
        authorization::{self, authorization_server::token::tests::token},
        issuance::{
            self,
            credential_issuer::credential::tests::TEST_NONCE,
            credentials::tests::{create_test_template, credentials},
            offers::tests::offers,
        },
    };
    use serde_json::json;
    use tower::ServiceExt;

    /// This test calls the token status list endpoint which in turn calls the function above.
    /// The remainder of the test breaks down the Token Status List response in various steps and checks these steps one by one.
    #[tokio::test]
    pub async fn test_token_status_list() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_test_template(&library_state).await;

        let mut app = issuance::router((issuance_state.clone(), library_state));

        // We must create a signed credential first to initiate the status list creation. There is no other way we expose Status List creation through the endpoints.
        create_test_signed_credential(&mut app, &issuance_state).await;

        // Fetch the Status List Token
        let token_status_list_response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/ietf-oauth-token-status-list/0")
                    .header(http::header::ACCEPT, StatusListTokenResponseType::Jwt.to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(token_status_list_response.status(), StatusCode::OK);
        assert_eq!(
            token_status_list_response
                .headers()
                .get(http::header::CONTENT_TYPE)
                .unwrap(),
            "application/statuslist+jwt"
        );

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

        let decoded_jwt = decrypt_status_list_token(&jwt_status_list_token, decoding_key).unwrap();

        assert_eq!(jwt_header.typ.unwrap(), StatusListTyp::Jwt.to_string());
        assert_eq!(
            decoded_jwt.claims.sub,
            config().public_url.as_str().to_owned() + "ietf-oauth-token-status-list/0"
        );
        assert_eq!(decoded_jwt.claims.encoded_status_list.status_size, BITS_PER_STATUS);

        let status_list = decoded_jwt.claims.encoded_status_list.decode_decompress().unwrap();
        assert_eq!(status_list.len(), STATUS_LIST_BYTES_AMOUNT);
    }

    /// This test helper creates a signed credential with the following parameters:
    /// - format: jwt_vc_json
    /// - data model: VC DM 1.1
    /// - is_pre_authorized: true
    /// - with_anonymous_access: false
    /// - with_external_server: false
    /// - is_self_signed: false
    pub async fn create_test_signed_credential(app: &mut Router, issuance_state: &Arc<IssuanceState>) -> String {
        let command = agent_issuance::nonce::command::NonceCommand::GenerateNonce {
            c_nonce: TEST_NONCE.to_string(),
        };
        agent_shared::handlers::command_handler(TEST_NONCE, &issuance_state.command.nonce, command)
            .await
            .unwrap();

        let credential_configuration_id = "001".to_string();

        let credential_endpoint = credentials(app, &credential_configuration_id).await;

        let grants = offers(app, &credential_configuration_id).await.unwrap();

        let authorization_state =
            Arc::new(authorization_state(&InMemory, AuthorizationServices::default().await, Default::default()).await);
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut authorization_app = authorization::router((authorization_state, issuance_state.to_owned()));

        let access_token: String = token(&mut authorization_app, true, grants).await;
        let jwt = "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkzZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBYtgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ";

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {access_token}"))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credential_configuration_id": credential_configuration_id,
                            "proofs": {
                                "jwt":[jwt]
                            }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        credential_endpoint
    }
}
