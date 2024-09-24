use agent_shared::application_state::CommandHandler;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;

use crate::credential::aggregate::Credential;
use crate::credential::queries::all_credentials::AllHolderCredentialsView;
use crate::credential::queries::HolderCredentialView;
use crate::offer::aggregate::Offer;
use crate::offer::queries::all_offers::AllReceivedOffersView;
use crate::offer::queries::ReceivedOfferView;

#[derive(Clone)]
pub struct HolderState {
    pub command: CommandHandlers,
    pub query: Queries,
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
type Queries = ViewRepositories<
    dyn ViewRepository<HolderCredentialView, Credential>,
    dyn ViewRepository<AllHolderCredentialsView, Credential>,
    dyn ViewRepository<ReceivedOfferView, Offer>,
    dyn ViewRepository<AllReceivedOffersView, Offer>,
>;

pub struct ViewRepositories<C1, C2, O1, O2>
where
    C1: ViewRepository<HolderCredentialView, Credential> + ?Sized,
    C2: ViewRepository<AllHolderCredentialsView, Credential> + ?Sized,
    O1: ViewRepository<ReceivedOfferView, Offer> + ?Sized,
    O2: ViewRepository<AllReceivedOffersView, Offer> + ?Sized,
{
    pub holder_credential: Arc<C1>,
    pub all_holder_credentials: Arc<C2>,
    pub received_offer: Arc<O1>,
    pub all_received_offers: Arc<O2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            holder_credential: self.holder_credential.clone(),
            all_holder_credentials: self.all_holder_credentials.clone(),
            received_offer: self.received_offer.clone(),
            all_received_offers: self.all_received_offers.clone(),
        }
    }
}
