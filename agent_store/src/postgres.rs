use crate::{partition_event_publishers, EventPublisher, Partitions};
use agent_holder::{services::HolderServices, state::HolderState};
use agent_identity::{services::IdentityServices, state::IdentityState};
use agent_issuance::{
    offer::queries::{access_token::AccessTokenQuery, pre_authorized_code::PreAuthorizedCodeQuery},
    services::IssuanceServices,
    state::IssuanceState,
    SimpleLoggingQuery,
};
use agent_shared::{
    application_state::Command, config::config, custom_queries::ListAllQuery, generic_query::generic_query,
};
use agent_verification::{services::VerificationServices, state::VerificationState};
use async_trait::async_trait;
use cqrs_es::{Aggregate, Query};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};
use std::{collections::HashMap, sync::Arc};

struct AggregateHandler<A>
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
    fn new(pool: Pool<Postgres>, services: A::Services) -> Self {
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

pub async fn identity_state(
    identity_services: Arc<IdentityServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> IdentityState {
    let connection_string = config().event_store.connection_string.clone().expect(
        "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
    );
    let pool = default_postgress_pool(&connection_string).await;

    // Initialize the postgres repositories.
    let connection = Arc::new(PostgresViewRepository::new("connection", pool.clone()));
    let all_connections = Arc::new(PostgresViewRepository::new("all_connections", pool.clone()));
    let document = Arc::new(PostgresViewRepository::new("document", pool.clone()));
    let service = Arc::new(PostgresViewRepository::new("service", pool.clone()));
    let all_services = Arc::new(PostgresViewRepository::new("all_services", pool.clone()));

    // Create custom-queries for the offer aggregate.
    let all_connections_query = ListAllQuery::new(all_connections.clone(), "all_connections");
    let all_services_query = ListAllQuery::new(all_services.clone(), "all_services");

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        connection_event_publishers,
        document_event_publishers,
        service_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    IdentityState {
        command: agent_identity::state::CommandHandlers {
            connection: Arc::new(
                connection_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), identity_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(connection.clone()))
                        .append_query(all_connections_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            document: Arc::new(
                document_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), identity_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(document.clone())),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            service: Arc::new(
                service_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), identity_services)
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(service.clone()))
                        .append_query(all_services_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_identity::state::ViewRepositories {
            connection,
            all_connections,
            document,
            service,
            all_services,
        },
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

pub async fn holder_state(
    holder_services: Arc<HolderServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> HolderState {
    let connection_string = config().event_store.connection_string.clone().expect(
        "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
    );
    let pool = default_postgress_pool(&connection_string).await;

    // Initialize the postgres repositories.
    let holder_credential: Arc<PostgresViewRepository<_, _>> =
        Arc::new(PostgresViewRepository::new("holder_credential", pool.clone()));
    let all_holder_credentials: Arc<PostgresViewRepository<_, _>> =
        Arc::new(PostgresViewRepository::new("all_holder_credentials", pool.clone()));
    let presentation = Arc::new(PostgresViewRepository::new("presentation", pool.clone()));
    let all_presentations = Arc::new(PostgresViewRepository::new("all_presentations", pool.clone()));
    let received_offer = Arc::new(PostgresViewRepository::new("received_offer", pool.clone()));
    let all_received_offers = Arc::new(PostgresViewRepository::new("all_received_offers", pool.clone()));

    // Create custom-queries for the offer aggregate.
    let all_holder_credentials_query = ListAllQuery::new(all_holder_credentials.clone(), "all_holder_credentials");
    let all_presentations_query = ListAllQuery::new(all_presentations.clone(), "all_presentations");
    let all_received_offers_query = ListAllQuery::new(all_received_offers.clone(), "all_received_offers");

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        holder_credential_event_publishers,
        presentation_event_publishers,
        received_offer_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    HolderState {
        command: agent_holder::state::CommandHandlers {
            credential: Arc::new(
                holder_credential_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), holder_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(holder_credential.clone()))
                        .append_query(all_holder_credentials_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            presentation: Arc::new(
                presentation_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), holder_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(presentation.clone()))
                        .append_query(all_presentations_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            offer: Arc::new(
                received_offer_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool, holder_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(received_offer.clone()))
                        .append_query(all_received_offers_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_holder::state::ViewRepositories {
            holder_credential,
            all_holder_credentials,
            presentation,
            all_presentations,
            received_offer,
            all_received_offers,
        },
    }
}

pub async fn verification_state(
    verification_services: Arc<VerificationServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> VerificationState {
    let connection_string = config().event_store.connection_string.clone().expect(
        "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
    );
    let pool = default_postgress_pool(&connection_string).await;

    // Initialize the postgres repositories.
    let authorization_request = Arc::new(PostgresViewRepository::new("authorization_request", pool.clone()));
    let all_authorization_requests = Arc::new(PostgresViewRepository::new("all_authorization_requests", pool.clone()));
    let connection = Arc::new(PostgresViewRepository::new("connection", pool.clone()));

    // Create custom-queries for the offer aggregate.
    let all_authorization_requests_query =
        ListAllQuery::new(all_authorization_requests.clone(), "all_authorization_requests");

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        authorization_request_event_publishers,
        connection2_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    VerificationState {
        command: agent_verification::state::CommandHandlers {
            authorization_request: Arc::new(
                authorization_request_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool.clone(), verification_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(authorization_request.clone()))
                        .append_query(all_authorization_requests_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            connection: Arc::new(
                connection2_event_publishers.into_iter().fold(
                    AggregateHandler::new(pool, verification_services)
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(connection.clone())),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_verification::state::ViewRepositories {
            authorization_request,
            all_authorization_requests,
            connection,
        },
    }
}
