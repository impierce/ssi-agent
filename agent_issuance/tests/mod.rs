use agent_issuance::{
    command::{IssuanceCommand, Metadata},
    state::new_application_state,
};
use serde_json::json;

#[tokio::test]
async fn test() {
    let application_state = new_application_state().await;

    let command = IssuanceCommand::CreateCredentialData {
        credential: serde_json::json!({"first_name": "Ferris"}),
    };
    application_state
        .cqrs
        .execute("agg-id-F39A0C", command)
        .await
        .unwrap();
}
