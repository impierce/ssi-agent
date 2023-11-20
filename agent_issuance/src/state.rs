use crate::model::aggregate::IssuanceData;
use crate::queries::IssuanceDataView;
use crate::services::IssuanceServices;
use agent_store::state::ApplicationState;

pub async fn new_application_state() -> ApplicationState<IssuanceData, IssuanceDataView> {
    agent_store::state::application_state(
        // vec![Box::new(SimpleLoggingQuery {})],
        vec![],
        IssuanceServices {},
    )
    .await
}
