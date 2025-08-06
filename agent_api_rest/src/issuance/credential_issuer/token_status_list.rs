use agent_issuance::{credential::aggregate::CredentialStatus, state::IssuanceState};
use agent_shared::config::{
    config, get_preferred_did_method, get_preferred_signing_algorithm, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use http::StatusCode;
use hyper::header;
use oauth_tsl::{
    relying_party::StatusListTokenResponseType,
    status_list::{Bits, EncodedStatusList, StatusList, StatusType},
    tokens::status_list_token::{compress_gzip, StatusListToken, StatusListTokenClaims},
};
use oid4vc_core::jwt::encode;
use rand::Rng;
use tokio::task;

use crate::{handlers::query_handler, issuance::error::PublicError};

pub async fn token_status_list(
    State(state): State<IssuanceState>,
    Path(status_list_number): Path<usize>,
) -> Result<Response, PublicError> {
    let all_credentials = query_handler("all_credentials", &state.query.all_credentials)
        .await?
        .map(|all_credentials_view| all_credentials_view.credentials.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    let amount_indices = STATUS_LIST_BYTES_AMOUNT * 8 / BITS_PER_STATUS as usize;

    let lower_bound = status_list_number * amount_indices;
    let upper_bound = (status_list_number + 1) * amount_indices;

    let mut used_indices: Vec<CredentialStatus> = all_credentials
        .iter()
        .filter_map(|c| {
            let index = c.credential_status.index;
            if index >= lower_bound && index < upper_bound {
                Some(c.credential_status.clone())
            } else {
                None
            }
        })
        .collect();

    // This block ensures that the remaining empty 30% of a status list is filled with random values.
    // This block works in tandem with the part of `fn patch_credential` which only fills 70% of a status list.
    used_indices = task::spawn_blocking(move || -> Result<Vec<CredentialStatus>, PublicError> {
        let mut indices = used_indices.clone();

        let mut rng = rand::rng();
        while indices.len() < amount_indices {
            let random_index = rng.random_range(lower_bound..upper_bound);
            if !indices
                .iter()
                .any(|credential_status| credential_status.index == random_index)
            {
                // the range is 0..2 because BITS_PER_STATUS is set to 2, meaning 4 options, but we only have 3 options defined (VALID, UNVALID, SUSPENDED)
                let status_type = rng.random_range(0..2);
                indices.push(CredentialStatus {
                    index: random_index,
                    status: status_type.try_into().map_err(|_| PublicError::InternalServerError)?,
                });
            }
        }

        Ok(indices)
    })
    .await
    .map_err(|_| PublicError::InternalServerError)??;

    used_indices.sort_by_key(|credential_status| credential_status.index);

    let mut status_list = StatusList {
        status_size: Bits::try_from(BITS_PER_STATUS).map_err(|_| PublicError::InternalServerError)?,
        ..Default::default()
    };
    status_list
        .pack_statuses_into_bytes(used_indices.iter().map(|s| s.status).collect::<Vec<StatusType>>())
        .map_err(|_| PublicError::InternalServerError)?;

    let mut sub_url = config().ietf_oauth_token_status_list_uri.clone();
    sub_url
        .path_segments_mut()
        .map_err(|_| PublicError::InternalServerError)?
        .push(&status_list_number.to_string());

    let status_list_claims = StatusListTokenClaims {
        sub: sub_url.to_string(),
        iat: chrono::Utc::now().timestamp(),
        exp: None,
        ttl: None,
        encoded_status_list: EncodedStatusList::try_from(status_list).map_err(|_| PublicError::InternalServerError)?,
    };

    let mut status_list_token = StatusListToken {
        claims: status_list_claims,
        ..Default::default()
    };
    status_list_token.header.alg = get_preferred_signing_algorithm();
    let default_did_method = get_preferred_did_method().to_string();

    let jwt_token = encode(
        state.signer,
        status_list_token.header,
        status_list_token.claims,
        &default_did_method,
    )
    .await
    .map_err(|_| PublicError::InternalServerError)?;

    let compressed_jwt_token = compress_gzip(&jwt_token).map_err(|_| PublicError::InternalServerError)?;

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
    use agent_issuance::state::initialize;
    use agent_secret_manager::{service::Service, subject::Subject};
    use agent_shared::config::{config, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT};
    use agent_store::in_memory;
    use axum::body::{self, Body};
    use http::{Request, StatusCode};
    use jsonwebtoken::{decode_header, Algorithm, DecodingKey};
    use oauth_tsl::{
        relying_party::{decompress_gzip, decrypt_status_list_token, StatusListTokenResponseType},
        tokens::status_list_token::StatusListTyp,
    };
    use oid4vc_core::authentication::verify::Verify;

    use crate::issuance::router;
    use tower::Service as _;

    #[tokio::test]
    pub async fn test_token_status_list() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
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
