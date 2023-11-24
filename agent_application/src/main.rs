use agent_api_rest::app;
use agent_issuance::{
    command::IssuanceCommand, handlers::command_handler, model::aggregate::IssuanceData, queries::IssuanceDataView,
    services::IssuanceServices, state::DynApplicationState,
};
use agent_store::postgres::PostgresApplicationState;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let state = Arc::new(PostgresApplicationState::new(vec![], IssuanceServices {}).await)
        as DynApplicationState<IssuanceData, IssuanceDataView>;

    tokio::spawn(startup_events(state.clone()));

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}

async fn startup_events(state: DynApplicationState<IssuanceData, IssuanceDataView>) {
    println!("Starting up ...");

    // Create subject
    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::CreateSubject {
            pre_authorized_code: "pre-auth-code-1337".to_string(),
        },
    )
    .await
    {
        Ok(_) => println!("Subject created"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    // Load template
    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::LoadCredentialFormatTemplate {
            credential_format_template: serde_json::from_str(r#"{"foo":"bar"}"#).unwrap(),
        },
    )
    .await
    {
        Ok(_) => println!("Template loaded"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };
}
