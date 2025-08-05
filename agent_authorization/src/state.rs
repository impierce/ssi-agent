use agent_shared::application_state::CommandHandler;
use agent_shared::handlers::{command_handler, query_handler};
use cqrs_es::persist::ViewRepository;
use oid4vc_core::Sign;
use std::sync::Arc;
use tracing::{debug, info};

use crate::domain::access_token::aggregate::AccessToken;
use crate::domain::access_token::views::AccessTokenView;
use crate::domain::authorization_code::aggregate::AuthorizationCode;
use crate::domain::authorization_code::views::AuthorizationCodeView;
use crate::domain::client::aggregate::Client;
use crate::domain::client::command::ClientCommand;
use crate::domain::client::views::ClientView;
use crate::domain::consent::aggregate::Consent;
use crate::domain::consent::views::ConsentView;
use crate::domain::oauth2_authorization_request::aggregate::OAuth2AuthorizationRequest;
use crate::domain::oauth2_authorization_request::views::OAuth2AuthorizationRequestView;

#[derive(Clone)]
pub struct AuthorizationState {
    pub command: CommandHandlers,
    pub query: Queries,
    pub signer: Arc<dyn Sign>,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub client: CommandHandler<Client>,
    pub oauth2_authorization_request: CommandHandler<OAuth2AuthorizationRequest>,
    pub authorization_code: CommandHandler<AuthorizationCode>,
    pub access_token: CommandHandler<AccessToken>,
    pub consent: CommandHandler<Consent>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn ViewRepository<ClientView, Client>,
    dyn ViewRepository<OAuth2AuthorizationRequestView, OAuth2AuthorizationRequest>,
    dyn ViewRepository<AuthorizationCodeView, AuthorizationCode>,
    dyn ViewRepository<AccessTokenView, AccessToken>,
    dyn ViewRepository<ConsentView, Consent>,
>;

pub struct ViewRepositories<C, OAR, AC, AT, Co>
where
    C: ViewRepository<ClientView, Client> + ?Sized,
    OAR: ViewRepository<OAuth2AuthorizationRequestView, OAuth2AuthorizationRequest> + ?Sized,
    AC: ViewRepository<AuthorizationCodeView, AuthorizationCode> + ?Sized,
    AT: ViewRepository<AccessTokenView, AccessToken> + ?Sized,
    Co: ViewRepository<ConsentView, Consent> + ?Sized,
{
    pub client: Arc<C>,
    pub oauth2_authorization_request: Arc<OAR>,
    pub authorization_code: Arc<AC>,
    pub access_token: Arc<AT>,
    pub consent: Arc<Co>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            client: self.client.clone(),
            oauth2_authorization_request: self.oauth2_authorization_request.clone(),
            authorization_code: self.authorization_code.clone(),
            access_token: self.access_token.clone(),
            consent: self.consent.clone(),
        }
    }
}

/// Initialize the authorization state.
pub async fn initialize(state: &AuthorizationState) -> anyhow::Result<()> {
    info!("Initializing the authorization state ...");

    initialize_clients(state).await?;

    Ok(())
}

pub const UNIME_CLIENT_ID: &str = "unime-client-id";

/// Initialize the default client (UniMe) in the authorization state.
async fn initialize_clients(state: &AuthorizationState) -> anyhow::Result<()> {
    if let Some(client) = query_handler(UNIME_CLIENT_ID, &state.query.client).await? {
        debug!("UniMe client already exists: {:?}", client);
        Ok(())
    } else {
        let command = ClientCommand::RegisterClient {
            client_id: UNIME_CLIENT_ID.to_string(),
            client_secret: None,
            client_name: Some("UniMe".to_string()),
            logo_uri: Some("FIXME: add unime logo".to_string()),
            policy_uri: None,
            tos_uri: None,
            redirect_uris: vec![
                "https://website-git-feat-assetlinks-app-site-association-impierce.vercel.app/callback"
                    .parse()
                    .unwrap(),
            ],
            grant_types: vec![
                "authorization_code".to_string(),
                "urn:ietf:params:oauth:grant-type:pre-authorized_code".to_string(),
            ],
            response_types: vec!["code".to_string()],
            token_endpoint_auth_method: "none".to_string(),
            require_pkce: true,
            code_challenge_methods_supported: vec!["S256".to_string()],
            require_pushed_authorization_request: true,
        };

        command_handler(UNIME_CLIENT_ID, &state.command.client, command).await?;

        Ok(())
    }
}
