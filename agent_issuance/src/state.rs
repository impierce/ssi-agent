use crate::model::aggregate::Credential;
use crate::queries::TempQuery;
use crate::services::IssuanceServices;
use agent_store::state::ApplicationState;

pub async fn new_application_state() -> ApplicationState<Credential> {
    agent_store::state::application_state(TempQuery {}, IssuanceServices {}).await
}
