use agent_api_rest::app;
use agent_issuance::{
    command::IssuanceCommand, handlers::command_handler, model::aggregate::IssuanceData, queries::IssuanceDataView,
    services::IssuanceServices, state::ApplicationState,
};
use agent_store::postgres;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let state = Arc::new(postgres::ApplicationState::new(vec![], IssuanceServices {}).await)
        as ApplicationState<IssuanceData, IssuanceDataView>;

    tokio::spawn(startup_events(state.clone()));

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}

async fn startup_events(state: ApplicationState<IssuanceData, IssuanceDataView>) {
    let base_url: url::Url = "https://example.com/".parse().unwrap();

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::LoadCredentialFormatTemplate {
            credential_format_template: serde_json::from_str(r#"{"foo":"bar"}"#).unwrap(),
        },
    )
    .await
    {
        Ok(_) => println!("Startup task completed: `LoadCredentialFormatTemplate`"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::LoadAuthorizationServerMetadata {
            authorization_server_metadata: Box::new(AuthorizationServerMetadata {
                issuer: base_url.clone(),
                token_endpoint: Some(base_url.join("token").unwrap()),
                ..Default::default()
            }),
        },
    )
    .await
    {
        Ok(_) => println!("Startup task completed: `LoadAuthorizationServerMetadata`"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::LoadCredentialIssuerMetadata {
            credential_issuer_metadata: CredentialIssuerMetadata {
                credential_issuer: base_url.clone(),
                authorization_server: None,
                credential_endpoint: base_url.join("credential").unwrap(),
                deferred_credential_endpoint: None,
                batch_credential_endpoint: None,
                credentials_supported: vec![],
                display: None,
            },
        },
    )
    .await
    {
        Ok(_) => println!("Startup task completed: `LoadCredentialIssuerMetadata`"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::CreateSubject {
            pre_authorized_code: "adhjhdjajkdkhjhdj".to_string(),
        },
    )
    .await
    {
        Ok(_) => println!("Startup task completed."),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::CreateCredentialsSupported {
            credentials_supported: vec![serde_json::from_value(json!({
                "format": "jwt_vc_json",
                "cryptographic_binding_methods_supported": [
                    "did:key",
                ],
                "cryptographic_suites_supported": [
                    "EdDSA"
                ],
                "credential_definition":{
                    "type": [
                        "VerifiableCredential",
                        "OpenBadgeCredential"
                    ]
                },
                "proof_types_supported": [
                    "jwt"
                ]
            }
            ))
            .unwrap()],
        },
    )
    .await
    {
        Ok(_) => println!("Startup task completed."),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    // match command_handler(
    //     "agg-id-F39A0C".to_string(),
    //     &state,
    //     IssuanceCommand::CreateTokenResponse {
    //         subject_id: uuid::Uuid::new_v4(),
    //         token_request: TokenRequest::PreAuthorizedCode {
    //             pre_authorized_code: "adhjhdjajkdkhjhdj".to_string(),
    //             user_pin: None,
    //         },
    //     },
    // )
    // .await
    // {
    //     Ok(_) => println!("Startup task completed."),
    //     Err(err) => println!("Startup task failed: {:#?}", err),
    // };

    // match command_handler(
    //     "agg-id-F39A0C".to_string(),
    //     &state,
    //     IssuanceCommand::CreateTokenResponse {
    //         subject_id: uuid::Uuid::new_v4(),
    //         token_request: TokenRequest::PreAuthorizedCode {
    //             pre_authorized_code: "adhjhdjajkdkhjhdj".to_string(),
    //             user_pin: None,
    //         },
    //     },
    // )
    // .await
    // {
    //     Ok(_) => println!("Startup task completed."),
    //     Err(err) => println!("Startup task failed: {:#?}", err),
    // };
}
