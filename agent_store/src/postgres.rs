use crate::{partition_event_publishers, EventPublisher, EventStore, Partitions};
use agent_issuance::{
    offer::queries::{access_token::AccessTokenQuery, pre_authorized_code::PreAuthorizedCodeQuery},
    services::IssuanceServices,
    state::IssuanceState,
    SimpleLoggingQuery,
};
use agent_shared::{
    application_state::Command, config::config, custom_queries::ListAllQuery, generic_query::generic_query,
};
use async_trait::async_trait;
use cqrs_es::{persist::ViewRepository, Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::Pool;
use std::{collections::HashMap, sync::Arc};

pub struct AggregateHandler<A>
where
    A: Aggregate,
{
    pub cqrs: PostgresCqrs<A>,
}

#[async_trait]
impl<A> Command<A> for AggregateHandler<A>
where
    A: Aggregate + 'static,
    <A as Aggregate>::Command: Send + Sync,
{
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>> {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }
}

impl<A> AggregateHandler<A>
where
    A: Aggregate,
{
    fn new(pool: Pool<sqlx::Postgres>, services: A::Services) -> Self {
        Self {
            cqrs: postgres_es::postgres_cqrs(pool, vec![], services),
        }
    }

    fn append_query<Q>(self, query: Q) -> Self
    where
        Q: Query<A> + 'static,
    {
        Self {
            cqrs: self.cqrs.append_query(Box::new(query)),
        }
    }

    fn append_event_publisher(self, query: Box<dyn Query<A>>) -> Self {
        Self {
            cqrs: self.cqrs.append_query(query),
        }
    }
}

pub struct Postgres;

impl EventStore for Postgres {
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
        let pool = default_postgress_pool(&connection_string).await;

        let all_aggregates_name = format!("all_{}", A::aggregate_type());

        // Initialize the postgres repositories.
        let aggregate: Arc<PostgresViewRepository<V, A>> =
            Arc::new(PostgresViewRepository::<V, A>::new(&A::aggregate_type(), pool.clone()));
        let all_aggregates: Arc<PostgresViewRepository<AV, A>> =
            Arc::new(PostgresViewRepository::<AV, A>::new(&all_aggregates_name, pool.clone()));

        (
            Arc::new(
                event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), services)
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
    let connection_string = config().event_store.connection_string.clone().expect(
        "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
    );
    let pool = default_postgress_pool(&connection_string).await;

    // Initialize the postgres repositories.
    let server_config = Arc::new(PostgresViewRepository::new("server_config", pool.clone()));
    let pre_authorized_code = Arc::new(PostgresViewRepository::new("pre_authorized_code", pool.clone()));
    let access_token = Arc::new(PostgresViewRepository::new("access_token", pool.clone()));
    let credential = Arc::new(PostgresViewRepository::new("credential", pool.clone()));
    let all_credentials = Arc::new(PostgresViewRepository::new("all_credentials", pool.clone()));
    let offer = Arc::new(PostgresViewRepository::new("offer", pool.clone()));
    let all_offers = Arc::new(PostgresViewRepository::new("all_offers", pool.clone()));

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
                    AggregateHandler::new(pool.clone(), ())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(server_config.clone())),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            credential: Arc::new(
                credential_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), issuance_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(credential.clone()))
                        .append_query(all_credentials_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            offer: Arc::new(
                offer_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), issuance_services)
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
