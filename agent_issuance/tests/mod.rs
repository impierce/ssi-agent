use agent_issuance::{
    command::{IssuanceCommand, Metadata},
    state::new_application_state,
};

#[tokio::test]
async fn test() {
    let application_state = new_application_state().await;

    let command = IssuanceCommand::CreateCredentialData {
        credential_subject: serde_json::json!({"first_name": "Ferris"}),
        metadata: Metadata {
            credential_type: vec!["VerifiableCredential".into()],
        },
    };
    application_state
        .cqrs
        .execute("agg-id-F39A0C", command)
        .await
        .unwrap();
}
