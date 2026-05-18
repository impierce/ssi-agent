use crate::services::OAuth2AuthorizationRequestDomainServices;

use super::command::OAuth2AuthorizationRequestCommand;
use super::error::OAuth2AuthorizationRequestError;
use super::event::OAuth2AuthorizationRequestEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vci::{
    authorization_details::AuthorizationDetailsObject, authorization_request::CodeChallengeMethod, InteractionType,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use url::Url;

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub enum ConsentStatus {
    #[default]
    Pending,
    Granted,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuth2AuthorizationRequest {
    #[serde(rename = "id")]
    pub oauth2_authorization_request_id: String,
    pub response_type: String,
    pub state: String,
    pub client_id: String,
    pub redirect_uri: Option<Url>,
    pub scope: Option<String>,
    pub issuer_state: Option<String>,

    // OID4VCI
    pub authorization_details: Vec<AuthorizationDetailsObject>,

    // PKCE
    #[serde(default)]
    pub code_challenge: Option<String>,
    #[serde(default)]
    pub code_challenge_method: Option<CodeChallengeMethod>,

    pub expires_at: i64,
    pub consent_status: ConsentStatus,

    // TODO: This does not belong here, but with the current architecturefor now we need it to be able to link the
    // authorization request to the interaction session in the interactive authorization flow.
    pub openid4vp_presentation: Option<serde_json::Value>,
}

#[async_trait]
impl Aggregate for OAuth2AuthorizationRequest {
    type Command = OAuth2AuthorizationRequestCommand;
    type Event = OAuth2AuthorizationRequestEvent;
    type Error = OAuth2AuthorizationRequestError;
    type Services = OAuth2AuthorizationRequestDomainServices;

    fn aggregate_type() -> String {
        "oauth2_authorization_request".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use OAuth2AuthorizationRequestCommand::*;
        use OAuth2AuthorizationRequestEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateOAuth2AuthorizationRequest {
                oauth2_authorization_request_id,
                pushed_authorization_request,
                expires_at,
                interaction_type,
            } => {
                let openid4vp_presentation = if let Some(InteractionType::OpenId4VpPresentation) = interaction_type {
                    Some(
                        services
                            .openid4vp_presentation_service
                            .create_openid4vp_presentation_request()
                            .await
                            .expect("FIXME"),
                    )
                } else {
                    None
                };

                Ok(vec![OAuth2AuthorizationRequestCreated {
                    oauth2_authorization_request_id,
                    response_type: pushed_authorization_request.response_type,
                    // TODO: required or optional?
                    state: pushed_authorization_request.state.unwrap_or_default(),
                    client_id: pushed_authorization_request.client_id,
                    redirect_uri: pushed_authorization_request.redirect_uri,
                    scope: pushed_authorization_request.scope,
                    issuer_state: pushed_authorization_request.issuer_state,
                    authorization_details: pushed_authorization_request.authorization_details,
                    code_challenge: pushed_authorization_request.code_challenge,
                    code_challenge_method: pushed_authorization_request.code_challenge_method,
                    expires_at,
                    openid4vp_presentation,
                }])
            }
            GrantConsent => {
                let now = chrono::Utc::now().timestamp();
                if now > self.expires_at {
                    Ok(vec![OAuth2AuthorizationRequestExpired {
                        oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                        consent_status: ConsentStatus::Expired,
                    }])
                } else {
                    Ok(vec![ConsentGranted {
                        oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                        consent_status: ConsentStatus::Granted,
                    }])
                }
            }
            RejectConsent => {
                let now = chrono::Utc::now().timestamp();
                if now > self.expires_at {
                    Ok(vec![OAuth2AuthorizationRequestExpired {
                        oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                        consent_status: ConsentStatus::Expired,
                    }])
                } else {
                    Ok(vec![ConsentRejected {
                        oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                        consent_status: ConsentStatus::Rejected,
                    }])
                }
            }
            SubmitOpenId4VpResponse { openid4vp_response } => {
                println!("{}:{}", file!(), line!());
                services
                    .openid4vp_presentation_service()
                    .verify_openid4vp_response(openid4vp_response)
                    .await
                    .expect("FIXME");

                println!("{}:{}", file!(), line!());
                let now = chrono::Utc::now().timestamp();
                println!("{}:{}", file!(), line!());
                if now > self.expires_at {
                    println!("{}:{}", file!(), line!());
                    Ok(vec![OAuth2AuthorizationRequestExpired {
                        oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                        consent_status: ConsentStatus::Expired,
                    }])
                } else {
                    println!("{}:{}", file!(), line!());
                    Ok(vec![ConsentGranted {
                        oauth2_authorization_request_id: self.oauth2_authorization_request_id.clone(),
                        consent_status: ConsentStatus::Granted,
                    }])
                }
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OAuth2AuthorizationRequestEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            OAuth2AuthorizationRequestCreated {
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
                openid4vp_presentation,
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
                self.openid4vp_presentation = openid4vp_presentation;
            }
            OAuth2AuthorizationRequestExpired {
                oauth2_authorization_request_id,
                consent_status,
            } => {
                self.oauth2_authorization_request_id = oauth2_authorization_request_id;
                self.consent_status = consent_status;
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
    async fn test_create_oauth2_authorization_request(
        oauth2_authorization_request_id: String,
        pushed_authorization_request: AuthorizationRequest,
        expires_at: i64,
    ) {
        OAuth2AuthorizationRequestTestFramework::with(OAuth2AuthorizationRequestDomainServices::default())
            .given_no_previous_events()
            .when(OAuth2AuthorizationRequestCommand::CreateOAuth2AuthorizationRequest {
                oauth2_authorization_request_id: oauth2_authorization_request_id.clone(),
                pushed_authorization_request: pushed_authorization_request.clone(),
                expires_at,
                interaction_type: None,
            })
            .then_expect_events(vec![
                OAuth2AuthorizationRequestEvent::OAuth2AuthorizationRequestCreated {
                    oauth2_authorization_request_id,
                    response_type: pushed_authorization_request.response_type,
                    state: pushed_authorization_request.state.unwrap(),
                    client_id: pushed_authorization_request.client_id,
                    redirect_uri: pushed_authorization_request.redirect_uri,
                    scope: pushed_authorization_request.scope,
                    issuer_state: pushed_authorization_request.issuer_state,
                    authorization_details: pushed_authorization_request.authorization_details,
                    code_challenge: pushed_authorization_request.code_challenge,
                    code_challenge_method: pushed_authorization_request.code_challenge_method,
                    expires_at,
                    openid4vp_presentation: None,
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_grant_consent(
        oauth2_authorization_request_id: String,
        authorization_request_pushed_event: OAuth2AuthorizationRequestEvent,
    ) {
        OAuth2AuthorizationRequestTestFramework::with(OAuth2AuthorizationRequestDomainServices::default())
            .given(vec![authorization_request_pushed_event.clone()])
            .when(OAuth2AuthorizationRequestCommand::GrantConsent)
            .then_expect_events(vec![OAuth2AuthorizationRequestEvent::ConsentGranted {
                oauth2_authorization_request_id,
                consent_status: ConsentStatus::Granted,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_reject_consent(
        oauth2_authorization_request_id: String,
        authorization_request_pushed_event: OAuth2AuthorizationRequestEvent,
    ) {
        OAuth2AuthorizationRequestTestFramework::with(OAuth2AuthorizationRequestDomainServices::default())
            .given(vec![authorization_request_pushed_event.clone()])
            .when(OAuth2AuthorizationRequestCommand::RejectConsent)
            .then_expect_events(vec![OAuth2AuthorizationRequestEvent::ConsentRejected {
                oauth2_authorization_request_id,
                consent_status: ConsentStatus::Rejected,
            }]);
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(feature = "test_utils")]
pub mod test_utils {
    use std::sync::OnceLock;

    use super::*;
    use crate::domain::client::aggregate::test_utils::client_id;
    use oid4vci::{authorization_details::OpenidCredential, authorization_request::AuthorizationRequest, pkce};
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
        "https://client.example.test/cb".parse().ok()
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
            credential_configuration_id: "001".to_string(),
            credential_identifiers: None,
            claims: None,
        }]
    }

    static CODE_VERIFIER: OnceLock<Vec<u8>> = OnceLock::new();

    #[fixture]
    pub fn code_verifier() -> &'static [u8] {
        CODE_VERIFIER.get_or_init(|| pkce::code_verifier(128))
    }

    static CODE_CLALLENGE: OnceLock<String> = OnceLock::new();

    #[fixture]
    pub fn code_challenge() -> String {
        CODE_CLALLENGE
            .get_or_init(|| pkce::code_challenge(code_verifier()))
            .to_owned()
    }

    #[fixture]
    pub fn code_challenge_method() -> Option<CodeChallengeMethod> {
        Some(CodeChallengeMethod::S256)
    }

    #[fixture]
    pub fn expires_at() -> i64 {
        chrono::Utc::now().timestamp() + 3600
    }

    #[fixture]
    pub fn pushed_authorization_request(
        response_type: String,
        state: String,
        client_id: String,
        redirect_uri: Option<Url>,
        scope: String,
        issuer_state: Option<String>,
        authorization_details: Vec<AuthorizationDetailsObject>,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
    ) -> AuthorizationRequest {
        AuthorizationRequest {
            response_type,
            state: Some(state),
            client_id,
            redirect_uri,
            scope: Some(scope),
            issuer_state,
            authorization_details,
            code_challenge: Some(code_challenge),
            code_challenge_method,
        }
    }

    #[fixture]
    pub fn authorization_request_pushed_event(
        oauth2_authorization_request_id: String,
        pushed_authorization_request: AuthorizationRequest,
        expires_at: i64,
    ) -> OAuth2AuthorizationRequestEvent {
        OAuth2AuthorizationRequestEvent::OAuth2AuthorizationRequestCreated {
            oauth2_authorization_request_id,
            response_type: pushed_authorization_request.response_type,
            state: pushed_authorization_request.state.unwrap(),
            client_id: pushed_authorization_request.client_id,
            redirect_uri: pushed_authorization_request.redirect_uri,
            scope: pushed_authorization_request.scope,
            issuer_state: pushed_authorization_request.issuer_state,
            authorization_details: pushed_authorization_request.authorization_details,
            code_challenge: pushed_authorization_request.code_challenge,
            code_challenge_method: pushed_authorization_request.code_challenge_method,
            expires_at,
            openid4vp_presentation: None,
        }
    }
}
