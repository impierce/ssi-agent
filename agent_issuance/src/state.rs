use crate::command::IssuanceCommand;
use crate::model::aggregate::IssuanceData;
use crate::queries::IssuanceDataView;
use crate::services::IssuanceServices;
use agent_store::state::ApplicationState;
use cqrs_es::persist::ViewRepository;
use oid4vci::credential_issuer::authorization_server_metadata::AuthorizationServerMetadata;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;

pub async fn new_application_state() -> ApplicationState<IssuanceData, IssuanceDataView> {
    let state = agent_store::state::application_state(
        // vec![Box::new(SimpleLoggingQuery {})],
        vec![],
        IssuanceServices {},
    )
    .await;

    let base_url: url::Url = "https://example.com/".parse().unwrap();

    state
        .cqrs
        .execute(
            "agg-id-F39A0C",
            IssuanceCommand::LoadAuthorizationServerMetadata {
                authorization_server_metadata: AuthorizationServerMetadata {
                    issuer: base_url.clone(),
                    token_endpoint: Some(base_url.join("token").unwrap()),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();

    state
        .cqrs
        .execute(
            "agg-id-F39A0C",
            IssuanceCommand::LoadCredentialIssuerMetadata {
                credential_issuer_metadata: CredentialIssuerMetadata {
                    credential_issuer: base_url.clone(),
                    authorization_server: None,
                    credential_endpoint: base_url.join("credential").unwrap(),
                    deferred_credential_endpoint: None,
                    batch_credential_endpoint: Some(base_url.join("batch_credential").unwrap()),
                    credentials_supported: vec![],
                    display: None,
                },
            },
        )
        .await
        .unwrap();

    // state
    //     .cqrs
    //     .execute(
    //         "agg-id-F39A0C",
    //         IssuanceCommand::CreateCredentialsSupported {
    //             credentials_supported: vec![],
    //         },
    //     )
    //     .await
    //     .unwrap();

    // state
    //     .cqrs
    //     .execute("agg-id-F39A0C", IssuanceCommand::CreateCredentialOffer)
    //     .await
    //     .unwrap();

    // let view = state.issuance_data_query.load(&"agg-id-F39A0C").await.unwrap();

    // println!("view: {:#?}", view);

    state
}
