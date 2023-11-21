use crate::model::aggregate::Credential;
use crate::queries::CredentialView;
use crate::services::IssuanceServices;
use agent_store::state::ApplicationState;
use cqrs_es::mem_store::MemStore;
use cqrs_es::CqrsFramework;

pub async fn new_application_state() -> ApplicationState<Credential, CredentialView> {
    agent_store::state::application_state(
        // vec![Box::new(SimpleLoggingQuery {})],
        vec![],
        IssuanceServices {},
    )
    .await
}

pub async fn in_mem_state() -> ApplicationState<Credential, CredentialView> {
    let store = MemStore::<Credential>::default();
    let service = IssuanceServices {};
    let cqrs = CqrsFramework::new(store, vec![], service);
    cqrs
}
