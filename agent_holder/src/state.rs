use agent_shared::application_state::CommandHandler;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;

use crate::credential::aggregate::Credential;
use crate::credential::queries::CredentialView;
use crate::offer::aggregate::Offer;
use crate::offer::queries::OfferView;
use axum::extract::FromRef;

#[derive(Clone)]
pub struct HolderState {
    pub command: CommandHandlers,
    pub query: Queries,
}

impl<I, V> FromRef<(I, HolderState, V)> for HolderState {
    fn from_ref(application_state: &(I, HolderState, V)) -> HolderState {
        application_state.1.clone()
    }
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub credential: CommandHandler<Credential>,
    pub offer: CommandHandler<Offer>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<dyn ViewRepository<CredentialView, Credential>, dyn ViewRepository<OfferView, Offer>>;

pub struct ViewRepositories<C, O>
where
    C: ViewRepository<CredentialView, Credential> + ?Sized,
    O: ViewRepository<OfferView, Offer> + ?Sized,
{
    pub credential: Arc<C>,
    pub offer: Arc<O>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            credential: self.credential.clone(),
            offer: self.offer.clone(),
        }
    }
}
