use crate::managed_key::aggregate::ManagedKey;
use crate::managed_key::views::all_managed_keys::AllManagedKeysView;
use crate::managed_key::views::ManagedKeyView;
use agent_shared::application_state::CommandHandler;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;

#[derive(Clone)]
pub struct SecretManagerState {
    pub command: CommandHandlers,
    pub query: Queries,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub managed_key: CommandHandler<ManagedKey>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn ViewRepository<ManagedKeyView, ManagedKey>,
    dyn ViewRepository<AllManagedKeysView, ManagedKey>,
>;

pub struct ViewRepositories<MK1, MK2>
where
    MK1: ViewRepository<ManagedKeyView, ManagedKey> + ?Sized,
    MK2: ViewRepository<AllManagedKeysView, ManagedKey> + ?Sized,
{
    pub managed_key: Arc<MK1>,
    pub all_managed_keys: Arc<MK2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            managed_key: self.managed_key.clone(),
            all_managed_keys: self.all_managed_keys.clone(),
        }
    }
}
