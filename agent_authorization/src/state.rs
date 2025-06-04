use agent_shared::application_state::CommandHandler;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;
use tracing::{debug, info};

use crate::domain::access_token::aggregate::AccessToken;
use crate::domain::access_token::views::AccessTokenView;
use crate::domain::authorization_code::aggregate::AuthorizationCode;
use crate::domain::authorization_code::views::AuthorizationCodeView;
use crate::domain::client::aggregate::Client;
use crate::domain::client::views::ClientView;
use crate::domain::consent::aggregate::Consent;
use crate::domain::consent::views::ConsentView;
use crate::domain::oauth2_authorization_request::aggregate::OAuth2AuthorizationRequest;
use crate::domain::oauth2_authorization_request::views::OAuth2AuthorizationRequestView;

#[derive(Clone)]
pub struct AuthorizationState {
    pub command: CommandHandlers,
    pub query: Queries,
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
