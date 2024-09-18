use agent_shared::application_state::CommandHandler;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;

use crate::credential::aggregate::Credential;
use crate::credential::queries::all_credentials::AllCredentialsView;
use crate::credential::queries::CredentialView;
use crate::offer::aggregate::Offer;
use crate::offer::queries::all_offers::AllOffersView;
use crate::offer::queries::OfferView;
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
    dyn ViewRepository<CredentialView, Credential>,
    dyn ViewRepository<AllCredentialsView, Credential>,
    dyn ViewRepository<PresentationView, Presentation>,
    dyn ViewRepository<AllPresentationsView, Presentation>,
    dyn ViewRepository<OfferView, Offer>,
    dyn ViewRepository<AllOffersView, Offer>,
>;

pub struct ViewRepositories<C1, C2, P1, P2, O1, O2>
where
    C1: ViewRepository<CredentialView, Credential> + ?Sized,
    C2: ViewRepository<AllCredentialsView, Credential> + ?Sized,
    P1: ViewRepository<PresentationView, Presentation> + ?Sized,
    P2: ViewRepository<AllPresentationsView, Presentation> + ?Sized,
    O1: ViewRepository<OfferView, Offer> + ?Sized,
    O2: ViewRepository<AllOffersView, Offer> + ?Sized,
{
    pub credential: Arc<C1>,
    pub all_credentials: Arc<C2>,
    pub presentation: Arc<P1>,
    pub all_presentations: Arc<P2>,
    pub offer: Arc<O1>,
    pub all_offers: Arc<O2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            credential: self.credential.clone(),
            all_credentials: self.all_credentials.clone(),
            presentation: self.presentation.clone(),
            all_presentations: self.all_presentations.clone(),
            offer: self.offer.clone(),
            all_offers: self.all_offers.clone(),
        }
    }
}
