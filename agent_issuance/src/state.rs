use crate::model::aggregate::Credential;
use crate::queries::CredentialView;
use crate::services::IssuanceServices;
use agent_store::state::ApplicationState;

pub async fn new_application_state() -> ApplicationState<Credential, CredentialView> {
    agent_store::state::application_state(
        // vec![Box::new(SimpleLoggingQuery {})],
        vec![],
        IssuanceServices {},
    )
    .await
}
