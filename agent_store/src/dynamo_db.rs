use crate::{partition_event_publishers, AggregateHandler, EventPublisher, EventStoreTemp, Partitions};
use agent_issuance::{
    offer::queries::{access_token::AccessTokenQuery, pre_authorized_code::PreAuthorizedCodeQuery},
    services::IssuanceServices,
    state::IssuanceState,
    SimpleLoggingQuery,
};
use agent_shared::{application_state::Command, custom_queries::ListAllQuery, generic_query::generic_query};
use aws_sdk_dynamodb::{
    config::{Credentials, Region},
    Client, Config,
};
use cqrs_es::{
    persist::{PersistedEventStore, ViewRepository},
    Aggregate, Query, View,
};
use dynamo_es::{DynamoEventRepository, DynamoViewRepository};
use std::sync::Arc;

impl<A> AggregateHandler<A, PersistedEventStore<DynamoEventRepository, A>>
where
    A: Aggregate,
{
    fn new(client: Client, services: A::Services) -> Self {
        Self {
            cqrs: dynamo_es::dynamodb_cqrs(client, vec![], services),
        }
    }
}

pub struct DynamoDB;

impl EventStoreTemp for DynamoDB {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
    ) -> (
        Arc<dyn Command<A> + Send + Sync>,
        Arc<dyn ViewRepository<V, A>>,
        Arc<dyn ViewRepository<AV, A>>,
    )
    where
        <A as Aggregate>::Command: Send + Sync,
    {
        use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;

        // Create a connector that will be used to establish TLS connections
        let tls_connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1()
            .enable_http2()
            .build();

        // Create a hyper-based HTTP client that uses this TLS connector.
        let http_client = HyperClientBuilder::new().build(tls_connector);

        let credentials = Credentials::new("TESTAWSID", "TESTAWSKEY", None, None, "");
        let config = Config::builder()
            .behavior_version_latest()
            .region(Region::new("eu-central-1"))
            .endpoint_url("http://cqrs-dynamodb-db:8000")
            .credentials_provider(credentials)
            .http_client(http_client)
            .build();
        let client = Client::from_conf(config);

        let all_aggregates_name = format!("all_{}s", A::aggregate_type());

        // Initialize the postgres repositories.
        let aggregate: Arc<DynamoViewRepository<V, A>> =
            Arc::new(DynamoViewRepository::<V, A>::new(&A::aggregate_type(), client.clone()));
        let all_aggregates: Arc<DynamoViewRepository<AV, A>> =
            Arc::new(DynamoViewRepository::<AV, A>::new(&all_aggregates_name, client.clone()));

        (
            Arc::new(
                event_publishers.into_iter().fold(
                    AggregateHandler::new(client.clone(), services)
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(aggregate.clone()))
                        .append_query(ListAllQuery::new(all_aggregates.clone(), &all_aggregates_name)),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            aggregate,
            all_aggregates,
        )
    }
}

pub async fn issuance_state(
    issuance_services: Arc<IssuanceServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> IssuanceState {
    let region = Region::new("us-west-2");
    let credentials = Credentials::new("TESTAWSID", "TESTAWSKEY", None, None, "");
    let config = Config::builder()
        .behavior_version_latest()
        .region(region)
        .endpoint_url("http://localhost:8000")
        .credentials_provider(credentials)
        .build();
    let client = Client::from_conf(config);

    // Initialize the postgres repositories.
    let server_config = Arc::new(DynamoViewRepository::new("server_config", client.clone()));
    let pre_authorized_code = Arc::new(DynamoViewRepository::new("pre_authorized_code", client.clone()));
    let access_token = Arc::new(DynamoViewRepository::new("access_token", client.clone()));
    let credential = Arc::new(DynamoViewRepository::new("credential", client.clone()));
    let all_credentials = Arc::new(DynamoViewRepository::new("all_credentials", client.clone()));
    let offer = Arc::new(DynamoViewRepository::new("offer", client.clone()));
    let all_offers = Arc::new(DynamoViewRepository::new("all_offers", client.clone()));

    // Create custom-queries for the offer aggregate.
    let pre_authorized_code_query = PreAuthorizedCodeQuery::new(pre_authorized_code.clone());
    let access_token_query = AccessTokenQuery::new(access_token.clone());

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        server_config_event_publishers,
        credential_event_publishers,
        offer_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    // Create custom-queries for the offer aggregate.
    let all_credentials_query = ListAllQuery::new(all_credentials.clone(), "all_credentials");
    let all_offers_query = ListAllQuery::new(all_offers.clone(), "all_offers");

    IssuanceState {
        command: agent_issuance::state::CommandHandlers {
            server_config: Arc::new(
                server_config_event_publishers.into_iter().fold(
                    AggregateHandler::new(client.clone(), ())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(server_config.clone())),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            credential: Arc::new(
                credential_event_publishers.into_iter().fold(
                    AggregateHandler::new(client.clone(), issuance_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(credential.clone()))
                        .append_query(all_credentials_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            offer: Arc::new(
                offer_event_publishers.into_iter().fold(
                    AggregateHandler::new(client.clone(), issuance_services)
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(offer.clone()))
                        .append_query(all_offers_query)
                        .append_query(pre_authorized_code_query)
                        .append_query(access_token_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_issuance::state::ViewRepositories {
            server_config,
            pre_authorized_code,
            access_token,
            credential,
            all_credentials,
            offer,
            all_offers,
        },
    }
}
