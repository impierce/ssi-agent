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
use cqrs_es::persist::PersistedEventStore;
use cqrs_es::{persist::ViewRepository, Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresEventRepository, PostgresViewRepository};
use sqlx::Pool;
use std::sync::Arc;

impl<A> AggregateHandler<A, PersistedEventStore<PostgresEventRepository, A>>
where
    A: Aggregate,
{
    fn new(pool: Pool<sqlx::Postgres>, services: A::Services) -> Self {
        Self {
            cqrs: postgres_es::postgres_cqrs(pool, vec![], services),
        }
    }
}

pub struct Postgres {
    pub pool: Pool<sqlx::Postgres>,
}

impl Postgres {
    pub async fn new() -> Self {
        let connection_string = config().event_store.connection_string.clone().expect(
            "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
        );
        let pool = default_postgress_pool(&connection_string).await;
        Self { pool }
    }
    // TODO: Run [Pool::close] during graceful shutdown to close all open connections.
}

impl CqrsComponentBuilder for Postgres {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
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
        let all_aggregates_name = format!("all_{}s", A::aggregate_type());

        // Initialize the postgres repositories.
        let aggregate: Arc<PostgresViewRepository<V, A>> = Arc::new(PostgresViewRepository::<V, A>::new(
            &A::aggregate_type(),
            self.pool.clone(),
        ));
        let all_aggregates: Arc<PostgresViewRepository<AV, A>> = Arc::new(PostgresViewRepository::<AV, A>::new(
            &all_aggregates_name,
            self.pool.clone(),
        ));

        (
            Arc::new(AggregateHandler::new(self.pool.clone(), services).with_parameters(
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
    pool: Pool<sqlx::Postgres>,
    issuance_services: Arc<IssuanceServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> IssuanceState {
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
                    AggregateHandler::new(pool.clone(), issuance_services.clone())
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
        subject: issuance_services.issuer.clone(),
    }
}
