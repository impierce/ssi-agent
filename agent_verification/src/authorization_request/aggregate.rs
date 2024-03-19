use std::sync::Arc;

use agent_shared::config;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::{
    authorization_request::{ByReference, Object},
    scope::Scope,
};
use serde::{Deserialize, Serialize};
use siopv2::siopv2::SIOPv2;
use tracing::info;

use crate::services::VerificationServices;

use super::{command::AuthorizationRequestCommand, error::AuthorizationRequestError, event::AuthorizationRequestEvent};

pub type SIOPv2AuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<SIOPv2>>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AuthorizationRequest {
    authorization_request: Option<SIOPv2AuthorizationRequest>,
    form_url_encoded_authorization_request: Option<String>,
    signed_authorization_request_object: Option<String>,
}

#[async_trait]
impl Aggregate for AuthorizationRequest {
    type Command = AuthorizationRequestCommand;
    type Event = AuthorizationRequestEvent;
    type Error = AuthorizationRequestError;
    type Services = Arc<VerificationServices>;

    fn aggregate_type() -> String {
        "authorization_request".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use AuthorizationRequestCommand::*;
        use AuthorizationRequestEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateAuthorizationRequest {
                client_metadata,
                state,
                nonce,
            } => {
                let verifier = &services.verifier;
                let verifier_did = verifier.identifier().unwrap();

                let url = config!("url").unwrap();
                let request_uri = format!("{url}/siopv2/request/{state}").parse().unwrap();
                let redirect_uri = format!("{url}/siopv2/redirect").parse::<url::Url>().unwrap();

                let authorization_request = Box::new(
                    SIOPv2AuthorizationRequest::builder()
                        .client_id(verifier_did.clone())
                        .scope(Scope::openid())
                        .redirect_uri(redirect_uri)
                        .response_mode("direct_post".to_string())
                        .client_metadata(*client_metadata)
                        .state(state)
                        .nonce(nonce)
                        .build()
                        .unwrap(),
                );

                let form_url_encoded_authorization_request = oid4vc_core::authorization_request::AuthorizationRequest {
                    body: ByReference {
                        client_id: verifier_did,
                        request_uri,
                    },
                }
                .to_string();

                Ok(vec![
                    AuthorizationRequestCreated { authorization_request },
                    FormUrlEncodedAuthorizationRequestCreated {
                        form_url_encoded_authorization_request,
                    },
                ])
            }
            SignAuthorizationRequestObject => {
                let relying_party = &services.relying_party;

                // TODO: Add error handling
                let signed_authorization_request_object = relying_party
                    .encode(self.authorization_request.as_ref().unwrap())
                    .unwrap();

                Ok(vec![AuthorizationRequestObjectSigned {
                    signed_authorization_request_object,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use AuthorizationRequestEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            AuthorizationRequestCreated { authorization_request } => {
                self.authorization_request = Some(*authorization_request);
            }
            FormUrlEncodedAuthorizationRequestCreated {
                form_url_encoded_authorization_request,
            } => {
                self.form_url_encoded_authorization_request = Some(form_url_encoded_authorization_request);
            }
            AuthorizationRequestObjectSigned {
                signed_authorization_request_object,
            } => {
                self.signed_authorization_request_object = Some(signed_authorization_request_object);
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use agent_shared::secret_manager::secret_manager;
    use cqrs_es::test::TestFramework;
    use did_manager::SecretManager;
    use lazy_static::lazy_static;
    use oid4vc_core::Subject;
    use oid4vc_core::{client_metadata::ClientMetadata, DidMethod, SubjectSyntaxType};

    use crate::services::test_utils::test_verification_services;

    use super::*;

    type AuthorizationRequestTestFramework = TestFramework<AuthorizationRequest>;

    #[test]
    #[serial_test::serial]
    fn test_create_authorization_request() {
        let verification_services = test_verification_services();

        AuthorizationRequestTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(AuthorizationRequestCommand::CreateAuthorizationRequest {
                client_metadata: Box::new(CLIENT_METADATA.clone()),
                state: "state".to_string(),
                nonce: "nonce".to_string(),
            })
            .then_expect_events(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(SIOPV2_AUTHORIZATION_REQUEST.clone()),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: FORM_URL_ENCODED_AUTHORIZATION_REQUEST.clone(),
                },
            ]);
    }

    #[test]
    #[serial_test::serial]
    fn test_sign_authorization_request_object() {
        let verification_services = test_verification_services();

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(SIOPV2_AUTHORIZATION_REQUEST.clone()),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: FORM_URL_ENCODED_AUTHORIZATION_REQUEST.clone(),
                },
            ])
            .when(AuthorizationRequestCommand::SignAuthorizationRequestObject)
            .then_expect_events(vec![AuthorizationRequestEvent::AuthorizationRequestObjectSigned {
                signed_authorization_request_object: SIGNED_AUTHORIZATION_REQUEST_OBJECT.clone(),
            }]);
    }

    lazy_static! {
        static ref VERIFIER: SecretManager = futures::executor::block_on(async { secret_manager().await });
        static ref VERIFIER_DID: String = VERIFIER.identifier().unwrap();
        static ref REDIRECT_URI: url::Url = "https://my-domain.example.org/siopv2/redirect"
            .parse::<url::Url>()
            .unwrap();
        static ref CLIENT_METADATA: ClientMetadata = ClientMetadata::default().with_subject_syntax_types_supported(
            vec![SubjectSyntaxType::Did(DidMethod::from_str("did:test").unwrap()),]
        );
        pub static ref SIOPV2_AUTHORIZATION_REQUEST: SIOPv2AuthorizationRequest = SIOPv2AuthorizationRequest::builder()
            .client_id(VERIFIER_DID.clone())
            .scope(Scope::openid())
            .redirect_uri(REDIRECT_URI.clone())
            .response_mode("direct_post".to_string())
            .client_metadata(CLIENT_METADATA.clone())
            .nonce("nonce".to_string())
            .state("state".to_string())
            .build()
            .unwrap();
        static ref FORM_URL_ENCODED_AUTHORIZATION_REQUEST: String = "\
        siopv2://idtoken?\
            client_id=did%3Akey%3Az6MkiieyoLMSVsJAZv7Jje5wWSkDEymUgkyF8kbcrjZpX3qd&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Fsiopv2%2Frequest%2Fstate"
            .to_string();
        static ref SIGNED_AUTHORIZATION_REQUEST_OBJECT: String =
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2lp\
             ZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkI3o2TWtp\
             aWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCJ9.eyJ\
             jbGllbnRfaWQiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0R\
             FeW1VZ2t5RjhrYmNyalpwWDNxZCIsInJlZGlyZWN0X3VyaSI6Imh0dHBzOi8vbXk\
             tZG9tYWluLmV4YW1wbGUub3JnL3Npb3B2Mi9yZWRpcmVjdCIsInN0YXRlIjoic3R\
             hdGUiLCJyZXNwb25zZV90eXBlIjoiaWRfdG9rZW4iLCJzY29wZSI6Im9wZW5pZCI\
             sInJlc3BvbnNlX21vZGUiOiJkaXJlY3RfcG9zdCIsIm5vbmNlIjoibm9uY2UiLCJ\
             jbGllbnRfbWV0YWRhdGEiOnsic3ViamVjdF9zeW50YXhfdHlwZXNfc3VwcG9ydGV\
             kIjpbImRpZDp0ZXN0Il19fQ.vjE-9wDbWqN8tRtnpYRZR7umZWb7M8MEMRSei28B\
             0zmTMDJlXeEYFJaDwN4hGVgRXmkTmwD_Tg-xhsfcD8BMAw"
                .to_string();
    }
}
