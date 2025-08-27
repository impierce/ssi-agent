use crate::{partition_event_publishers, AggregateHandler, CqrsComponentBuilder, EventPublisher, Partitions};
use agent_issuance::{
    offer::queries::{access_token::AccessTokenQuery, pre_authorized_code::PreAuthorizedCodeQuery},
    services::IssuanceServices,
    state::IssuanceState,
    SimpleLoggingQuery,
};
use agent_shared::{
    application_state::Command, config::config, custom_queries::ListAllQuery, generic_query::generic_query,
};
use cqrs_es::{persist::PersistedEventStore, CqrsFramework};
use cqrs_es::{persist::ViewRepository, Aggregate, Query, View};
use mongo_es::{Client, MongoEventRepository, MongoViewRepository};
use std::sync::Arc;

impl<A> AggregateHandler<A, PersistedEventStore<MongoEventRepository, A>>
where
    A: Aggregate,
{
    fn new(client: Client, services: A::Services) -> Self {
        let repo = MongoEventRepository::new(client);
        let store = PersistedEventStore::new_event_store(repo);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
        }
    }
}

pub struct MongoDB;

impl CqrsComponentBuilder for MongoDB {
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
        let connection_string = config().event_store.connection_string.clone().expect(
            "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
        );

        let client = mongodb::Client::with_uri_str(&connection_string)
            .await
            .expect("Failed to connect to MongoDB");

        let all_aggregates_name = format!("all_{}s", A::aggregate_type());

        // Initialize the mongo repositories.
        let aggregate: Arc<MongoViewRepository<V, A>> =
            Arc::new(MongoViewRepository::new(&A::aggregate_type(), client.clone()));
        let all_aggregates: Arc<MongoViewRepository<AV, A>> =
            Arc::new(MongoViewRepository::new(&all_aggregates_name, client.clone()));

        (
            Arc::new(AggregateHandler::new(client, services).with_parameters(
                aggregate.clone(),
                all_aggregates.clone(),
                event_publishers,
                &all_aggregates_name,
            )),
            aggregate,
            all_aggregates,
        )
    }
}

// TODO: make a generic function for this and move it to `lib.rs`.
pub async fn issuance_state(
    issuance_services: Arc<IssuanceServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> IssuanceState {
    let connection_string = config().event_store.connection_string.clone().expect(
        "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
    );
    let client = mongodb::Client::with_uri_str(&connection_string)
        .await
        .expect("Failed to connect to MongoDB");

    // Initialize the mongo repositories.
    let server_config = Arc::new(MongoViewRepository::new("server_config", client.clone()));
    let pre_authorized_code = Arc::new(MongoViewRepository::new("pre_authorized_code", client.clone()));
    let access_token = Arc::new(MongoViewRepository::new("access_token", client.clone()));
    let credential = Arc::new(MongoViewRepository::new("credential", client.clone()));
    let all_credentials = Arc::new(MongoViewRepository::new("all_credentials", client.clone()));
    let offer = Arc::new(MongoViewRepository::new("offer", client.clone()));
    let all_offers = Arc::new(MongoViewRepository::new("all_offers", client.clone()));

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
                    AggregateHandler::new(client.clone(), issuance_services.clone())
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
        signer: issuance_services.issuer.clone(),
    }
}
