use agent_shared::{
    config::{get_preferred_did_method, get_preferred_signing_algorithm},
    handlers::query_handler,
};
use oauth_tsl::{
    status_list::EncodedStatusList,
    tokens::status_list_token::{compress_gzip, StatusListToken, StatusListTokenClaims},
};
use oid4vc_core::jwt::encode;

use crate::{credential::aggregate::get_status_list_url, state::IssuanceState, status_list::error::StatusListError};

pub struct TokenStatusListService {}

impl TokenStatusListService {
    /// This function creates a gzip compressed JWT token containing a status list for credentials.
    /// The status list is created just-in-time based on a slice of the credentials queried from the store.
    /// This slice is in turn derived from the status list number specified in the last path segment of the url used to call the endpoint.
    /// The status list is then filled up to 30% with random values to enhance privacy and security.
    /// At last the remaining steps are executed according to the OAuth TSL specification to encode and compress the Status List Token.
    pub async fn create_gzip_status_list_jwt_token(
        self,
        status_list_id: String,
        state: &IssuanceState,
    ) -> Result<Vec<u8>, StatusListError> {
        let status_list = query_handler(&status_list_id, &state.query.status_list)
            .await
            .map_err(|_| StatusListError::StatusListQueryError)?
            .ok_or(StatusListError::StatusListNotFound(status_list_id.clone()))?;

        let sub_url = get_status_list_url(status_list_id).map_err(|_| StatusListError::StatusListUrlParsingError)?;

        let status_list_claims = StatusListTokenClaims {
            sub: sub_url.to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: None,
            ttl: None,
            encoded_status_list: EncodedStatusList::try_from(status_list.list)
                .map_err(StatusListError::StatusListEncodingError)?,
        };

        let mut status_list_token = StatusListToken {
            claims: status_list_claims,
            ..Default::default()
        };

        status_list_token.header.alg = get_preferred_signing_algorithm();
        let default_did_method = get_preferred_did_method().to_string();

        let jwt_token = encode(
            state.subject.clone(),
            status_list_token.header,
            status_list_token.claims,
            &default_did_method,
        )
        .await
        .map_err(|_| StatusListError::JwtEncodeError)?;

        let compressed_jwt_token = compress_gzip(&jwt_token).map_err(|_| StatusListError::GzipCompressionError)?;

        Ok(compressed_jwt_token)
    }
}
