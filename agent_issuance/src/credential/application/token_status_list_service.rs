use agent_shared::{
    config::{
        config, get_preferred_did_method, get_preferred_signing_algorithm, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT,
    },
    handlers::query_handler,
};
use oauth_tsl::{
    error::OAuthTSLError,
    status_list::{EncodedStatusList, StatusType},
    tokens::status_list_token::{compress_gzip, StatusListToken, StatusListTokenClaims},
};
use oid4vc_core::jwt::encode;
use rand::Rng;
use thiserror::Error;

use crate::state::IssuanceState;

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
    ) -> Result<Vec<u8>, TokenStatusListError> {
        let mut status_list = query_handler(&status_list_id, &state.query.status_list)
            .await
            .map_err(|_| TokenStatusListError::StatusListQueryError)?
            .ok_or(TokenStatusListError::StatusListNotFound(status_list_id.clone()))?;

        let amount_indices = STATUS_LIST_BYTES_AMOUNT * 8 / BITS_PER_STATUS as usize;

        // TODO move this block to a separate function in the library oauth_tsl. Argument should be the indices and either
        // - the status_list size from which we can derive that the difference is to be filled with random values,
        // - or the percentage of random values desired (e.g. 30%). One is from a payload/latency perspective, the other from a privacy/security perspective.
        //
        // This block ensures that the remaining empty 30% or more of a status list is filled with random values.
        // This block works in tandem with the part of `fn patch_credential` which only fills 70% of a status list.

        for i in 0..amount_indices {
            if !status_list.used_indices.contains(&i) {
                // rng must be initialized here otherwise errors occur with axum due to thread unsafe problems and the Send trait
                let mut rng = rand::rng();
                // the range is 0..2 because BITS_PER_STATUS is set to 2, meaning 4 options, but we only have 3 options defined (VALID, UNVALID, SUSPENDED)
                status_list.list.status_list.insert(i, rng.random_range(0..2));
            }
        }

        // TODO move whats below to a builder function in the library oauth_tsl.
        // - sensible default bitsize = 2
        // - sensible default random percentage = 30%
        // - amount of statuses is derived from the input vector of indices

        let statusses: Vec<StatusType> = status_list
            .list
            .status_list
            .iter()
            .map(|status| StatusType::try_from(*status))
            .collect::<Result<Vec<_>, _>>()
            .map_err(TokenStatusListError::StatusTypeError)?;

        // TODO: check if this whole status packing thing makes sense
        status_list
            .list
            .pack_statuses_into_bytes(statusses)
            .map_err(TokenStatusListError::InvalidStatusSize)?;

        let mut sub_url = config().ietf_oauth_token_status_list_uri.clone();
        sub_url
            .path_segments_mut()
            .map_err(|_| TokenStatusListError::SubUrlParsingError)?
            .push(&status_list_id.to_string());

        let status_list_claims = StatusListTokenClaims {
            sub: sub_url.to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: None,
            ttl: None,
            encoded_status_list: EncodedStatusList::try_from(status_list.list)
                .map_err(TokenStatusListError::StatusListEncodingError)?,
        };

        let mut status_list_token = StatusListToken {
            claims: status_list_claims,
            ..Default::default()
        };

        // This remaining block stays in ssi-agent
        status_list_token.header.alg = get_preferred_signing_algorithm();
        let default_did_method = get_preferred_did_method().to_string();

        let jwt_token = encode(
            state.subject.clone(),
            status_list_token.header,
            status_list_token.claims,
            &default_did_method,
        )
        .await
        .map_err(|_| TokenStatusListError::JwtEncodeError)?;

        let compressed_jwt_token = compress_gzip(&jwt_token).map_err(|_| TokenStatusListError::GzipCompressionError)?;

        Ok(compressed_jwt_token)
    }
}

#[derive(Error, Debug)]
pub enum TokenStatusListError {
    #[error("Failed to encode and compress the status list claim: {0:?}")]
    StatusListEncodingError(OAuthTSLError),
    #[error("Failed to convert/parse status type: {0:?}")]
    StatusTypeError(OAuthTSLError),
    #[error("Invalid status size: {0:?}")]
    InvalidStatusSize(OAuthTSLError),
    #[error("Failed to parse the `sub` url for the status list")]
    SubUrlParsingError,
    #[error("Failed to encode the status list token as JWT.")]
    JwtEncodeError,
    #[error("Failed to Gzip compress the JWT token.")]
    GzipCompressionError,
    #[error("Error querying the status list")]
    StatusListQueryError,
    #[error("Status list not found for the provided id: {0}")]
    StatusListNotFound(String),
}
