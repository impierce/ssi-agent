use agent_secret_manager::signer::SubjectSigner;
use agent_shared::{
    config::{get_preferred_did_method, get_preferred_signing_algorithm},
    handlers::public_query_handler,
};
use identity_kit::sd_jwt_vc::StatusListToken;

use crate::{credential::aggregate::get_status_list_url, state::IssuanceState, status_list::error::StatusListError};

pub struct TokenStatusListService {}

impl TokenStatusListService {
    /// This function creates a JWT token containing a status list for credentials.
    /// The status list is created just-in-time based on a slice of the credentials queried from the store.
    /// This slice is in turn derived from the status list number specified in the last path segment of the url used to call the endpoint.
    /// The status list is then filled up to 30% with random values to enhance privacy and security.
    /// At last the remaining steps are executed according to the OAuth TSL specification to encode the Status List Token.
    pub async fn create_status_list_jwt_token(
        self,
        status_list_id: String,
        state: &IssuanceState,
    ) -> Result<String, StatusListError> {
        let status_list = public_query_handler(&status_list_id, &state.query.status_list)
            .await
            .map_err(|_| StatusListError::StatusListQueryError)?
            .ok_or(StatusListError::StatusListNotFound(status_list_id.clone()))?;

        let sub_url = get_status_list_url(status_list_id).map_err(|_| StatusListError::StatusListUrlParsingError)?;

        let status_list_token = StatusListToken::new(sub_url.to_string(), chrono::Utc::now(), status_list.list);

        let signer = SubjectSigner::new(
            state.subject.clone(),
            get_preferred_did_method().to_string(),
            get_preferred_signing_algorithm(),
        )
        .await
        .ok_or(StatusListError::JwtEncodeError)?;

        let status_list_jwt = status_list_token
            .sign(&signer)
            .await
            .map_err(|_| StatusListError::JwtEncodeError)?;

        Ok(status_list_jwt.as_str().to_owned())
    }
}
