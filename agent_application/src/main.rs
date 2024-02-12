use std::sync::Arc;

use agent_api_rest::app;
use agent_issuance::{
    credential::{aggregate::Credential, queries::CredentialView, services::CredentialServices},
    handlers::{command_handler, query_handler},
    offer::{
        aggregate::Offer,
        command::OfferCommand,
        queries::{AccessTokenView, OfferSubQuery, OfferView, PreAuthorizedCodeView},
        services::OfferServices,
    },
    server_config::{self, aggregate::ServerConfig, queries::ServerConfigView, services::ServerConfigServices},
    startup_commands::startup_commands_server_config,
    state::{ApplicationState, CQRS},
};
use agent_shared::config;
use agent_store::in_memory;
use cqrs_es::{
    mem_store::MemStore,
    persist::{GenericQuery, ViewRepository},
    Aggregate, CqrsFramework, View,
};
use lazy_static::lazy_static;

lazy_static! {
    static ref HOST: url::Url = format!("http://{}:3033/", config!("host").unwrap()).parse().unwrap();
}

#[tokio::main]
async fn main() {
    let state = match config!("event_store").unwrap().as_str() {
        // "postgres" => postgres::ApplicationState::new(vec![Box::new(OfferSubQuery {})], IssuanceServices {}).await,
        _ => in_memory::application_state().await,
    };

    match config!("log_format").unwrap().as_str() {
        "json" => tracing_subscriber::fmt().json().init(),
        _ => tracing_subscriber::fmt::init(),
    }

    // initialize(state.clone(), startup_commands_server_config(HOST.clone())).await;

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(app_state).into_make_service())
        .await
        .unwrap();
}

#[tokio::test]
async fn test() {
    let application_state = in_memory::application_state().await;

    // let credential_aggregate_handler = CredentialAggregateHandler::new(Arc::new(MemRepository::new())).await;
    // let offer_aggregate_handler = OfferAggregateHandler::new(Arc::new(MemRepository::new())).await;
    // let server_config_aggregate_handler = ServerConfigAggregateHandler::new(Arc::new(MemRepository::new())).await;

    // let app_state = AppState {
    //     credential_aggregate_handler,
    //     offer_aggregate_handler,
    //     server_config_aggregate_handler,
    // };

    // let command = OfferCommand::CreateOffer;

    // app_state
    //     .offer_aggregate_handler
    //     .cqrs
    //     .execute("OFFER-0001", command)
    //     .await
    //     .unwrap();

    // let view = app_state
    //     .offer_aggregate_handler
    //     .main_view
    //     .load("OFFER-0001")
    //     .await
    //     .unwrap();

    // println!("{:?}", view);
    // let code = view.clone().unwrap().pre_authorized_code.clone();
    // println!("code: {:?}", code);
    // let token = view.unwrap().access_token.clone();
    // println!("token: {:?}", token);

    // let view = app_state
    //     .offer_aggregate_handler
    //     .pre_authorized_code_repo
    //     .load(&code)
    //     .await
    //     .unwrap();

    // println!("code here???: {:?}", view);

    // let view = app_state
    //     .offer_aggregate_handler
    //     .access_token_repo
    //     .load(&token)
    //     .await
    //     .unwrap();

    // println!("token here???: {:?}", view);

    // let offer = app_state
    //     .offer_aggregate_handler
    //     .main_view
    //     .load(&view.unwrap().offer_id)
    //     .await
    //     .unwrap();

    // println!("offer here???: {:?}", offer);
}
