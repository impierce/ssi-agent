use std::sync::Arc;

use agent_issuance::{
    credential::application::token_status_list_service::TokenStatusListService, state::IssuanceState,
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
    Path(status_list_number): Path<usize>,
) -> Result<Response, PublicError> {
    let token_status_list_service = TokenStatusListService {};
    let compressed_jwt_token = token_status_list_service
        .create_gzip_status_list_jwt_token(status_list_number, &state)
        .await
        .map_err(|_| PublicError::InternalServerError)?;

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

    use agent_issuance::state::initialize;
    use agent_secret_manager::{service::Service, subject::Subject};
    use agent_shared::config::{config, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT};
    use agent_store::{in_memory::InMemory, issuance_state};
    use axum::body::{self, Body};
    use http::{Request, StatusCode};
    use jsonwebtoken::{decode_header, Algorithm, DecodingKey};
    use oauth_tsl::{
        relying_party::{decompress_gzip, decrypt_status_list_token, StatusListTokenResponseType},
        tokens::status_list_token::StatusListTyp,
    };
    use oid4vc_core::authentication::verify::Verify;

    use crate::v0::issuance::router;
    use tower::Service as _;

    /// This test calls the token status list endpoint which in turn calls the function above.
    /// The remainder of the test breaks down the Token Status List response in various steps and checks these steps one by one.
    #[tokio::test]
    pub async fn test_token_status_list() {
        let issuance_state = Arc::new(issuance_state(&InMemory, Service::default(), Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let relying_party_state = Subject::default();

        let mut app = router(issuance_state);

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
}
