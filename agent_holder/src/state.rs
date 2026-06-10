use agent_shared::application_state::CommandHandler;
use agent_shared::view_repository::DynViewRepository;
use std::sync::Arc;

use crate::credential::aggregate::Credential;
use crate::credential::queries::all_credentials::AllHolderCredentialsView;
use crate::credential::queries::HolderCredentialView;
use crate::offer::aggregate::Offer;
use crate::offer::queries::all_offers::AllReceivedOffersView;
use crate::offer::queries::ReceivedOfferView;
use crate::presentation::aggregate::Presentation;
use crate::presentation::views::all_presentations::AllPresentationsView;
use crate::presentation::views::PresentationView;

#[derive(Clone)]
pub struct HolderState {
    pub command: CommandHandlers,
    pub query: Queries,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub credential: CommandHandler<Credential>,
    pub presentation: CommandHandler<Presentation>,
    pub offer: CommandHandler<Offer>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn DynViewRepository<HolderCredentialView, Credential>,
    dyn DynViewRepository<AllHolderCredentialsView, Credential>,
    dyn DynViewRepository<PresentationView, Presentation>,
    dyn DynViewRepository<AllPresentationsView, Presentation>,
    dyn DynViewRepository<ReceivedOfferView, Offer>,
    dyn DynViewRepository<AllReceivedOffersView, Offer>,
>;

pub struct ViewRepositories<C1, C2, P1, P2, O1, O2>
where
    C1: DynViewRepository<HolderCredentialView, Credential> + ?Sized,
    C2: DynViewRepository<AllHolderCredentialsView, Credential> + ?Sized,
    P1: DynViewRepository<PresentationView, Presentation> + ?Sized,
    P2: DynViewRepository<AllPresentationsView, Presentation> + ?Sized,
    O1: DynViewRepository<ReceivedOfferView, Offer> + ?Sized,
    O2: DynViewRepository<AllReceivedOffersView, Offer> + ?Sized,
{
    pub holder_credential: Arc<C1>,
    pub all_holder_credentials: Arc<C2>,
    pub presentation: Arc<P1>,
    pub all_presentations: Arc<P2>,
    pub received_offer: Arc<O1>,
    pub all_received_offers: Arc<O2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            holder_credential: self.holder_credential.clone(),
            all_holder_credentials: self.all_holder_credentials.clone(),
            presentation: self.presentation.clone(),
            all_presentations: self.all_presentations.clone(),
            received_offer: self.received_offer.clone(),
            all_received_offers: self.all_received_offers.clone(),
        }
    }
}
