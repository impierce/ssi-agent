use agent_api_rest::app;
use agent_issuance::{
    command::IssuanceCommand, handlers::command_handler, model::aggregate::Credential, queries::CredentialView,
    state::new_application_state,
};
use agent_store::state::ApplicationState;

#[tokio::main]
async fn main() {
    let state = new_application_state().await;

    tokio::spawn(startup_events(state.clone()));

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}

async fn startup_events(state: ApplicationState<Credential, CredentialView>) {
    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::LoadCredentialTemplate {
            credential_template: serde_json::from_str(r#"{"foo":"bar"}"#).unwrap(),
        },
    )
    .await
    {
        Ok(_) => println!("Startup task completed."),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };
}
