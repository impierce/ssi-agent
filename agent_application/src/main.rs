use agent_api_rest::app;
use agent_issuance::{
    command::IssuanceCommand, handlers::command_handler, init::load_templates, model::aggregate::IssuanceData,
    queries::IssuanceDataView, services::IssuanceServices, state::DynApplicationState,
};
use agent_store::postgres::PostgresApplicationState;
use std::sync::Arc;
use tracing::{debug, info};

#[tokio::main]
async fn main() {
    let state = Arc::new(PostgresApplicationState::new(vec![], IssuanceServices {}).await)
        as DynApplicationState<IssuanceData, IssuanceDataView>;

    // Release
    // tracing_subscriber::fmt().json().init();

    // Develop
    tracing_subscriber::fmt::init();

    tokio::spawn(startup_events(state.clone()));

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}

async fn startup_events(state: DynApplicationState<IssuanceData, IssuanceDataView>) {
    info!("Starting up ...");

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
        Ok(_) => info!("Subject created"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    // Load templates
    load_templates(&state).await;
}
