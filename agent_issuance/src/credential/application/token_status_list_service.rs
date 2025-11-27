use agent_shared::{
    config::{
        config, get_preferred_did_method, get_preferred_signing_algorithm, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT,
    },
    handlers::query_handler,
};
use cqrs_es::persist::PersistenceError;
use oauth_tsl::{
    error::OAuthTSLError,
    status_list::{Bits, EncodedStatusList, StatusList, StatusType},
    tokens::status_list_token::{compress_gzip, StatusListToken, StatusListTokenClaims},
};
use oid4vc_core::jwt::encode;
use rand::Rng;
use thiserror::Error;

use crate::{credential::aggregate::CredentialStatus, state::IssuanceState};

pub struct TokenStatusListService {}

impl TokenStatusListService {
    /// This function creates a gzip compressed JWT token containing a status list for credentials.
    /// The status list is created just-in-time based on a slice of the credentials queried from the store.
    /// This slice is in turn derived from the status list number specified in the last path segment of the url used to call the endpoint.
    /// The status list is then filled up to 30% with random values to enhance privacy and security.
    /// At last the remaining steps are executed according to the OAuth TSL specification to encode and compress the Status List Token.
    pub async fn create_gzip_status_list_jwt_token(
        self,
        status_list_number: usize,
        state: IssuanceState,
    ) -> Result<Vec<u8>, TokenStatusListError> {
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

        // TODO move this block to a separate function in the library oauth_tsl. Argument should be the indices and either
        // - the status_list size from which we can derive that the difference is to be filled with random values,
        // - or the percentage of random values desired (e.g. 30%). One is from a payload/latency perspective, the other from a privacy/security perspective.

        // This block ensures that the remaining empty 30% of a status list is filled with random values.
        // This block works in tandem with the part of `fn patch_credential` which only fills 70% of a status list.
        used_indices = {
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
                        status: status_type.try_into().map_err(TokenStatusListError::StatusTypeError)?,
                    });
                }
            }

            indices
        };

        used_indices.sort_by_key(|credential_status| credential_status.index);

        // TODO move whats below to a builder function in the library oauth_tsl.
        // - sensible default bitsize = 2
        // - sensible default random percentage = 30%
        // - amount of statuses is derived from the input vector of indices

        let mut status_list = StatusList {
            status_size: Bits::try_from(BITS_PER_STATUS).map_err(TokenStatusListError::InvalidStatusSize)?,
            ..Default::default()
        };
        status_list
            .pack_statuses_into_bytes(used_indices.iter().map(|s| s.status).collect::<Vec<StatusType>>())
            .map_err(TokenStatusListError::InvalidStatusSize)?;

        let mut sub_url = config().ietf_oauth_token_status_list_uri.clone();
        sub_url
            .path_segments_mut()
            .map_err(|_| TokenStatusListError::SubUrlParsingError)?
            .push(&status_list_number.to_string());

        let status_list_claims = StatusListTokenClaims {
            sub: sub_url.to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: None,
            ttl: None,
            encoded_status_list: EncodedStatusList::try_from(status_list)
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
            state.subject,
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
    #[error("Failed to query credentials in order to create the status list token: {0}")]
    QueryCredentialsError(#[from] PersistenceError),
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
}
