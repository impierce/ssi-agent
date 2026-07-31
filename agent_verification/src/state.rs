use agent_shared::application_state::CommandHandler;
use shared_kernel::authorization::{AuthorizationChecker, QueryOperation};
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;

use crate::authorization_request::aggregate::AuthorizationRequest;
use crate::authorization_request::views::all_authorization_requests::AllAuthorizationRequestsView;
use crate::authorization_request::views::AuthorizationRequestView;

impl QueryOperation for AuthorizationRequestView {
    const OPERATION_NAME: &'static str = "verification.authorization_requests.get";
}

impl QueryOperation for AllAuthorizationRequestsView {
    const OPERATION_NAME: &'static str = "verification.authorization_requests.list";
}

#[derive(Clone)]
pub struct VerificationState {
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    pub command: CommandHandlers,
    pub query: Queries,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub authorization_request: CommandHandler<AuthorizationRequest>,
}
/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn DynViewRepository<AuthorizationRequestView, AuthorizationRequest>,
    dyn DynViewRepository<AllAuthorizationRequestsView, AuthorizationRequest>,
>;

pub struct ViewRepositories<AR1, AR2>
where
    AR1: DynViewRepository<AuthorizationRequestView, AuthorizationRequest> + ?Sized,
    AR2: DynViewRepository<AllAuthorizationRequestsView, AuthorizationRequest> + ?Sized,
{
    pub authorization_request: Arc<AR1>,
    pub all_authorization_requests: Arc<AR2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            authorization_request: self.authorization_request.clone(),
            all_authorization_requests: self.all_authorization_requests.clone(),
        }
    }
}
