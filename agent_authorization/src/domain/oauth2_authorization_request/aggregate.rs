use super::command::OAuth2AuthorizationRequestCommand;
use super::error::OAuth2AuthorizationRequestError;
use super::event::OAuth2AuthorizationRequestEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct OAuth2AuthorizationRequest {
    #[serde(rename = "id")]
    pub oauth2_authorization_request_id: String,
    pub response_type: String,
    pub state: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    #[serde(default)]
    pub client_assertion_type: Option<String>,
    #[serde(default)]
    pub client_assertion: Option<String>,
    pub issuer_state: Option<String>,

    // OID4VCI
    // pub authorization_details: AuthorizationDetailsObject,

    // PKCE
    #[serde(default)]
    pub code_challenge: Option<String>,
    #[serde(default)]
    pub code_challenge_method: Option<String>,

    pub expires_at: i64,
    // FIXME?
    // status: USED/EXPIRED
}

#[async_trait]
impl Aggregate for OAuth2AuthorizationRequest {
    type Command = OAuth2AuthorizationRequestCommand;
    type Event = OAuth2AuthorizationRequestEvent;
    type Error = OAuth2AuthorizationRequestError;
    type Services = ();

    fn aggregate_type() -> String {
        "oauth2_authorization_request".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use OAuth2AuthorizationRequestCommand::*;
        use OAuth2AuthorizationRequestError::*;
        use OAuth2AuthorizationRequestEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            InitializeFromPushedAuthorizationRequest {
                oauth2_authorization_request_id,
                pushed_authorization_request,
                expires_at,
            } => {
                // Here you would implement the logic to initialize from the Pushed Authorization Request
                // For now, we return a dummy event
                Ok(vec![AuthorizationRequestPushed {
                    oauth2_authorization_request_id,
                    response_type: pushed_authorization_request.response_type,
                    state: pushed_authorization_request.state,
                    client_id: pushed_authorization_request.client_id,
                    redirect_uri: pushed_authorization_request.redirect_uri.to_string(),
                    scope: pushed_authorization_request.scope,
                    client_assertion_type: pushed_authorization_request.client_assertion_type,
                    client_assertion: pushed_authorization_request.client_assertion,
                    issuer_state: pushed_authorization_request.issuer_state,
                    code_challenge: pushed_authorization_request.code_challenge,
                    code_challenge_method: pushed_authorization_request.code_challenge_method,
                    expires_at,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OAuth2AuthorizationRequestEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            AuthorizationRequestPushed {
                oauth2_authorization_request_id,
                response_type,
                state,
                client_id,
                redirect_uri,
                scope,
                client_assertion_type,
                client_assertion,
                issuer_state,
                code_challenge,
                code_challenge_method,
                expires_at,
            } => {
                self.oauth2_authorization_request_id = oauth2_authorization_request_id;
                self.response_type = response_type;
                self.state = state;
                self.client_id = client_id;
                self.redirect_uri = redirect_uri;
                self.scope = scope;
                self.client_assertion_type = client_assertion_type;
                self.client_assertion = client_assertion;
                self.issuer_state = issuer_state;
                self.code_challenge = code_challenge;
                self.code_challenge_method = code_challenge_method;
                self.expires_at = expires_at;
            }
        }
    }
}

#[cfg(test)]
pub mod oauth2_authorization_request_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
