use super::command::ClientCommand;
use super::error::ClientError;
use super::event::ClientEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Client {
    #[serde(rename = "id")]
    pub client_id: String,
    /// The secret for confidential clients. `None` for public clients.
    pub client_secret: Option<String>,

    // --- Display & Legal Metadata ---
    /// The human-readable name of the client application.
    pub client_name: Option<String>,
    /// URL of the client's logo.
    pub logo_uri: Option<String>,
    /// URL of the client's policy document.
    pub policy_uri: Option<String>,
    /// URL of the client's terms of service.
    pub tos_uri: Option<String>,

    // --- OAuth 2.0 Core Configuration ---
    /// List of allowed redirect URIs for the authorization code flow.
    pub redirect_uris: Vec<Url>,
    /// Grant types the client is allowed to use (e.g., "authorization_code").
    pub grant_types: Vec<String>,
    /// Response types the client is allowed to request (e.g., "code").
    pub response_types: Vec<String>,
    /// The authentication method for the token endpoint (e.g., "none", "client_secret_basic").
    pub token_endpoint_auth_method: String,

    // --- Security Policies ---
    /// If `true`, the server will reject authorization requests that do not use PKCE.
    pub require_pkce: bool,
    /// A list of PKCE code challenge methods supported by the client (e.g., "S256").
    pub code_challenge_methods_supported: Vec<String>,
    /// If `true`, the server will reject authorization requests that do not use Pushed Authorization Requests (PAR).
    pub require_pushed_authorization_request: bool,
}

#[async_trait]
impl Aggregate for Client {
    type Command = ClientCommand;
    type Event = ClientEvent;
    type Error = ClientError;
    type Services = ();

    fn aggregate_type() -> String {
        "client".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use ClientCommand::*;
        use ClientEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            RegisterClient {
                client_id,
                client_secret,
                client_name,
                logo_uri,
                policy_uri,
                tos_uri,
                redirect_uris,
                grant_types,
                response_types,
                token_endpoint_auth_method,
                require_pkce,
                code_challenge_methods_supported,
                require_pushed_authorization_request,
            } => {
                // TODO: Validate the command parameters here.

                Ok(vec![ClientRegistered {
                    client_id,
                    client_secret,
                    client_name,
                    logo_uri,
                    policy_uri,
                    tos_uri,
                    redirect_uris,
                    grant_types,
                    response_types,
                    token_endpoint_auth_method,
                    require_pkce,
                    code_challenge_methods_supported,
                    require_pushed_authorization_request,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ClientEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            ClientRegistered {
                client_id,
                client_secret,
                client_name,
                logo_uri,
                policy_uri,
                tos_uri,
                redirect_uris,
                grant_types,
                response_types,
                token_endpoint_auth_method,
                require_pkce,
                code_challenge_methods_supported,
                require_pushed_authorization_request,
            } => {
                self.client_id = client_id;
                self.client_secret = client_secret;
                self.client_name = client_name;
                self.logo_uri = logo_uri;
                self.policy_uri = policy_uri;
                self.tos_uri = tos_uri;
                self.redirect_uris = redirect_uris;
                self.grant_types = grant_types;
                self.response_types = response_types;
                self.token_endpoint_auth_method = token_endpoint_auth_method;
                self.require_pkce = require_pkce;
                self.code_challenge_methods_supported = code_challenge_methods_supported;
                self.require_pushed_authorization_request = require_pushed_authorization_request;
            }
        }
    }
}

#[cfg(test)]
pub mod client_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type ClientTestFramework = TestFramework<Client>;

    #[rstest]
    #[serial_test::serial]
    async fn test_register_client(
        client_id: String,
        client_secret: Option<String>,
        client_name: Option<String>,
        logo_uri: Option<String>,
        policy_uri: Option<String>,
        tos_uri: Option<String>,
        redirect_uris: Vec<Url>,
        grant_types: Vec<String>,
        response_types: Vec<String>,
        token_endpoint_auth_method: String,
        require_pkce: bool,
        code_challenge_methods_supported: Vec<String>,
        require_pushed_authorization_request: bool,
    ) {
        ClientTestFramework::with(())
            .given_no_previous_events()
            .when(ClientCommand::RegisterClient {
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
                client_name: client_name.clone(),
                logo_uri: logo_uri.clone(),
                policy_uri: policy_uri.clone(),
                tos_uri: tos_uri.clone(),
                redirect_uris: redirect_uris.clone(),
                grant_types: grant_types.clone(),
                response_types: response_types.clone(),
                token_endpoint_auth_method: token_endpoint_auth_method.clone(),
                require_pkce,
                code_challenge_methods_supported: code_challenge_methods_supported.clone(),
                require_pushed_authorization_request,
            })
            .then_expect_events(vec![ClientEvent::ClientRegistered {
                client_id,
                client_secret,
                client_name,
                logo_uri,
                policy_uri,
                tos_uri,
                redirect_uris,
                grant_types,
                response_types,
                token_endpoint_auth_method,
                require_pkce,
                code_challenge_methods_supported,
                require_pushed_authorization_request,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use rstest::*;

    #[fixture]
    pub fn client_id() -> String {
        "client_id".to_string()
    }

    #[fixture]
    pub fn client_secret() -> Option<String> {
        // Test as a public client by default
        None
    }

    #[fixture]
    pub fn client_name() -> Option<String> {
        Some("Test Client Application".to_string())
    }

    #[fixture]
    pub fn logo_uri() -> Option<String> {
        Some("https://client.example.test/logo.png".to_string())
    }

    #[fixture]
    pub fn policy_uri() -> Option<String> {
        Some("https://client.example.test/policy".to_string())
    }

    #[fixture]
    pub fn tos_uri() -> Option<String> {
        Some("https://client.example.test/tos".to_string())
    }

    #[fixture]
    pub fn redirect_uris() -> Vec<Url> {
        vec!["https://client.example.test/cb".parse().unwrap()]
    }

    #[fixture]
    pub fn grant_types() -> Vec<String> {
        vec![
            "authorization_code".to_string(),
            "urn:ietf:params:oauth:grant-type:pre-authorized_code".to_string(),
        ]
    }

    #[fixture]
    pub fn response_types() -> Vec<String> {
        vec!["code".to_string()]
    }

    #[fixture]
    pub fn token_endpoint_auth_method() -> String {
        "none".to_string()
    }

    #[fixture]
    pub fn require_pkce() -> bool {
        true
    }

    #[fixture]
    pub fn code_challenge_methods_supported() -> Vec<String> {
        vec!["S256".to_string()]
    }

    #[fixture]
    pub fn require_pushed_authorization_request() -> bool {
        true
    }
}
