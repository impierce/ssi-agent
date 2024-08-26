use agent_shared::application_state::CommandHandler;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;

use crate::authorization_request::aggregate::AuthorizationRequest;
use crate::authorization_request::queries::AuthorizationRequestView;
use crate::connection::aggregate::Connection;
use crate::connection::queries::ConnectionView;

use axum::extract::FromRef;

#[derive(Clone)]
pub struct VerificationState {
    pub command: CommandHandlers,
    pub query: Queries,
}

impl<I, H> FromRef<(I, H, VerificationState)> for VerificationState {
    fn from_ref(application_state: &(I, H, VerificationState)) -> VerificationState {
        application_state.2.clone()
    }
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub authorization_request: CommandHandler<AuthorizationRequest>,
    pub connection: CommandHandler<Connection>,
}
/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn ViewRepository<ConnectionView, Connection>,
    dyn ViewRepository<AuthorizationRequestView, AuthorizationRequest>,
>;

pub struct ViewRepositories<C, AR>
where
    AR: ViewRepository<AuthorizationRequestView, AuthorizationRequest> + ?Sized,
    C: ViewRepository<ConnectionView, Connection> + ?Sized,
{
    pub authorization_request: Arc<AR>,
    pub connection: Arc<C>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            authorization_request: self.authorization_request.clone(),
            connection: self.connection.clone(),
        }
    }
}
