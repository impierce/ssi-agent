use super::command::OAuth2AuthorizationRequestCommand;
use super::error::OAuth2AuthorizationRequestError;
use super::event::OAuth2AuthorizationRequestEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use oid4vci::authorization_details::AuthorizationDetailsObject;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use url::Url;

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub enum ConsentStatus {
    #[default]
    Pending,
    Given,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct OAuth2AuthorizationRequest {
    #[serde(rename = "id")]
    pub oauth2_authorization_request_id: String,
    pub response_type: String,
    pub state: String,
    pub client_id: String,
    pub redirect_uri: Option<Url>,
    pub scope: String,
    pub issuer_state: Option<String>,

    // OID4VCI
    pub authorization_details: Vec<AuthorizationDetailsObject>,

    // PKCE
    #[serde(default)]
    pub code_challenge: Option<String>,
    #[serde(default)]
    pub code_challenge_method: Option<String>,

    pub expires_at: i64,
    // FIXME?
    // status: USED/EXPIRED
    pub consent_status: ConsentStatus,
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
            } => Ok(vec![AuthorizationRequestPushed {
                oauth2_authorization_request_id,
                response_type: pushed_authorization_request.response_type,
                state: pushed_authorization_request.state.expect("FIXME"),
                client_id: pushed_authorization_request.client_id,
                redirect_uri: pushed_authorization_request.redirect_uri,
                scope: pushed_authorization_request.scope.expect("FIXME"),
                issuer_state: pushed_authorization_request.issuer_state,
                authorization_details: pushed_authorization_request.authorization_details,
                code_challenge: pushed_authorization_request.code_challenge,
                code_challenge_method: pushed_authorization_request.code_challenge_method,
                expires_at,
            }]),
            GrantConsent => {
                if self.consent_status != ConsentStatus::Pending {
                    todo!("FIXME: Handle already given or rejected consent");
                    // return Err(ConsentAlreadyGivenOrRejected);
                }

                println!("Granting consent for request: {}", self.oauth2_authorization_request_id);

                Ok(vec![ConsentGranted {
                    oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                    consent_status: ConsentStatus::Given,
                }])
            }
            RejectConsent => {
                if self.consent_status != ConsentStatus::Pending {
                    todo!("FIXME: Handle already given or rejected consent");
                    // return Err(ConsentAlreadyGivenOrRejected);
                }

                println!(
                    "Rejecting consent for request: {}",
                    self.oauth2_authorization_request_id
                );

                Ok(vec![ConsentRejected {
                    oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                    consent_status: ConsentStatus::Rejected,
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
                issuer_state,
                authorization_details,
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
                self.issuer_state = issuer_state;
                self.authorization_details = authorization_details;
                self.code_challenge = code_challenge;
                self.code_challenge_method = code_challenge_method;
                self.expires_at = expires_at;
            }
            ConsentGranted {
                oauth2_authorization_request_id,
                consent_status,
            } => {
                self.oauth2_authorization_request_id = oauth2_authorization_request_id;
                self.consent_status = consent_status;
            }
            ConsentRejected {
                oauth2_authorization_request_id,
                consent_status,
            } => {
                self.oauth2_authorization_request_id = oauth2_authorization_request_id;
                self.consent_status = consent_status;
            }
        }
    }
}

#[cfg(test)]
pub mod oauth2_authorization_request_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use oid4vci::authorization_request::AuthorizationRequest;
    use rstest::rstest;

    type OAuth2AuthorizationRequestTestFramework = TestFramework<OAuth2AuthorizationRequest>;

    #[rstest]
    #[serial_test::serial]
    async fn test_initialize_from_par(
        oauth2_authorization_request_id: String,
        pushed_authorization_request: AuthorizationRequest,
        expires_at: i64,
    ) {
        OAuth2AuthorizationRequestTestFramework::with(())
            .given_no_previous_events()
            .when(
                OAuth2AuthorizationRequestCommand::InitializeFromPushedAuthorizationRequest {
                    oauth2_authorization_request_id: oauth2_authorization_request_id.clone(),
                    pushed_authorization_request: pushed_authorization_request.clone(),
                    expires_at,
                },
            )
            .then_expect_events(vec![OAuth2AuthorizationRequestEvent::AuthorizationRequestPushed {
                oauth2_authorization_request_id,
                response_type: pushed_authorization_request.response_type,
                state: pushed_authorization_request.state.unwrap(),
                client_id: pushed_authorization_request.client_id,
                redirect_uri: pushed_authorization_request.redirect_uri,
                scope: pushed_authorization_request.scope.unwrap(),
                issuer_state: pushed_authorization_request.issuer_state,
                authorization_details: pushed_authorization_request.authorization_details,
                code_challenge: pushed_authorization_request.code_challenge,
                code_challenge_method: pushed_authorization_request.code_challenge_method,
                expires_at,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_grant_consent(
        oauth2_authorization_request_id: String,
        authorization_request_pushed_event: OAuth2AuthorizationRequestEvent,
    ) {
        OAuth2AuthorizationRequestTestFramework::with(())
            .given(vec![authorization_request_pushed_event.clone()])
            .when(OAuth2AuthorizationRequestCommand::GrantConsent)
            .then_expect_events(vec![OAuth2AuthorizationRequestEvent::ConsentGranted {
                oauth2_authorization_request_id,
                consent_status: ConsentStatus::Given,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_reject_consent(
        oauth2_authorization_request_id: String,
        authorization_request_pushed_event: OAuth2AuthorizationRequestEvent,
    ) {
        OAuth2AuthorizationRequestTestFramework::with(())
            .given(vec![authorization_request_pushed_event.clone()])
            .when(OAuth2AuthorizationRequestCommand::RejectConsent)
            .then_expect_events(vec![OAuth2AuthorizationRequestEvent::ConsentRejected {
                oauth2_authorization_request_id,
                consent_status: ConsentStatus::Rejected,
            }]);
    }

    // #[rstest]
    // #[serial_test::serial]
    // async fn test_cannot_grant_consent_twice(authorization_request_pushed_event: OAuth2AuthorizationRequestEvent) {
    //     let request_id = match authorization_request_pushed_event.clone() {
    //         OAuth2AuthorizationRequestEvent::AuthorizationRequestPushed {
    //             oauth2_authorization_request_id,
    //             ..
    //         } => oauth2_authorization_request_id,
    //         _ => panic!("Wrong event type"),
    //     };

    //     let consent_granted_event = OAuth2AuthorizationRequestEvent::ConsentGranted {
    //         oauth2_authorization_request_id: request_id,
    //         consent_status: ConsentStatus::Given,
    //     };

    //     OAuth2AuthorizationRequestTestFramework::with(())
    //         .given(vec![authorization_request_pushed_event, consent_granted_event])
    //         .when(OAuth2AuthorizationRequestCommand::GrantConsent)
    //         .then_expect_error_message("FIXME: Handle already given or rejected consent");
    // }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use crate::domain::client::aggregate::test_utils::client_id;
    use oid4vci::{
        authorization_details::{CredentialConfigurationOrFormat, OpenidCredential},
        authorization_request::AuthorizationRequest,
    };
    use rstest::*;

    #[fixture]
    pub fn oauth2_authorization_request_id() -> String {
        "oauth2_authorization_request_id".to_string()
    }

    #[fixture]
    pub fn response_type() -> String {
        "code".to_string()
    }

    #[fixture]
    pub fn state() -> String {
        "state".to_string()
    }

    #[fixture]
    pub fn redirect_uri() -> Option<Url> {
        "https://client.example.com/cb".parse().ok()
    }

    #[fixture]
    pub fn scope() -> String {
        "openid profile".to_string()
    }

    #[fixture]
    pub fn issuer_state() -> Option<String> {
        Some("issuer_state_def".to_string())
    }

    #[fixture]
    pub fn authorization_details() -> Vec<AuthorizationDetailsObject> {
        vec![AuthorizationDetailsObject {
            r#type: OpenidCredential::Type,
            locations: None,
            credential_configuration_or_format: CredentialConfigurationOrFormat::CredentialConfigurationId {
                credential_configuration_id: "FIXME".to_string(),
                parameters: None,
            },
            claims: None,
        }]
    }

    #[fixture]
    pub fn code_challenge() -> Option<String> {
        Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string())
    }

    #[fixture]
    pub fn code_challenge_method() -> Option<String> {
        Some("S256".to_string())
    }

    #[fixture]
    pub fn expires_at() -> i64 {
        chrono::Utc::now().timestamp() + 3600
    }

    #[fixture]
    #[allow(clippy::too_many_arguments)]
    pub fn pushed_authorization_request(
        response_type: String,
        state: String,
        client_id: String,
        redirect_uri: Option<Url>,
        scope: String,
        issuer_state: Option<String>,
        authorization_details: Vec<AuthorizationDetailsObject>,
        code_challenge: Option<String>,
        code_challenge_method: Option<String>,
    ) -> AuthorizationRequest {
        AuthorizationRequest {
            response_type,
            state: Some(state),
            client_id,
            redirect_uri,
            scope: Some(scope),
            issuer_state,
            authorization_details,
            code_challenge,
            code_challenge_method,
        }
    }

    #[fixture]
    pub fn authorization_request_pushed_event(
        oauth2_authorization_request_id: String,
        pushed_authorization_request: AuthorizationRequest,
        expires_at: i64,
    ) -> OAuth2AuthorizationRequestEvent {
        OAuth2AuthorizationRequestEvent::AuthorizationRequestPushed {
            oauth2_authorization_request_id,
            response_type: pushed_authorization_request.response_type,
            state: pushed_authorization_request.state.unwrap(),
            client_id: pushed_authorization_request.client_id,
            redirect_uri: pushed_authorization_request.redirect_uri,
            scope: pushed_authorization_request.scope.unwrap(),
            issuer_state: pushed_authorization_request.issuer_state,
            authorization_details: pushed_authorization_request.authorization_details,
            code_challenge: pushed_authorization_request.code_challenge,
            code_challenge_method: pushed_authorization_request.code_challenge_method,
            expires_at,
        }
    }
}
