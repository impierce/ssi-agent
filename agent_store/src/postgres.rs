use agent_issuance::{
    credential::services::CredentialServices,
    offer::{
        aggregate::Offer,
        queries::{AccessTokenView, OfferSubQuery, PreAuthorizedCodeView},
        services::OfferServices,
    },
    server_config::services::ServerConfigServices,
    state::{generic_query, ApplicationState, CommandHandlers, ViewRepositories, CQRS},
};
use agent_shared::config;
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
impl<A> CQRS<A> for AggregateHandler<A>
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
}

pub async fn application_state() -> agent_issuance::state::ApplicationState {
    let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;

    // Initialize the postgres repositories.
    let server_config = Arc::new(PostgresViewRepository::new("server_config", pool.clone()));
    let credential = Arc::new(PostgresViewRepository::new("credential", pool.clone()));
    let offer = Arc::new(PostgresViewRepository::new("offer", pool.clone()));
    let pre_authorized_code = Arc::new(PostgresViewRepository::new("pre_authorized_code", pool.clone()));
    let access_token = Arc::new(PostgresViewRepository::new("access_token", pool.clone()));

    // Create sub-queries for the offer aggregate.
    let pre_authorized_code_query: OfferSubQuery<
        PostgresViewRepository<PreAuthorizedCodeView, Offer>,
        PreAuthorizedCodeView,
    > = OfferSubQuery::new(pre_authorized_code.clone(), "pre-authorized_code".to_string());
    let access_token_query: OfferSubQuery<PostgresViewRepository<AccessTokenView, Offer>, AccessTokenView> =
        OfferSubQuery::new(access_token.clone(), "access_token".to_string());

    ApplicationState {
        command: CommandHandlers {
            server_config: Arc::new(
                AggregateHandler::new(pool.clone(), ServerConfigServices)
                    .append_query(generic_query(server_config.clone())),
            ),
            credential: Arc::new(
                AggregateHandler::new(pool.clone(), CredentialServices).append_query(generic_query(credential.clone())),
            ),
            offer: Arc::new(
                AggregateHandler::new(pool, OfferServices)
                    .append_query(generic_query(offer.clone()))
                    .append_query(pre_authorized_code_query)
                    .append_query(access_token_query),
            ),
        },
        query: ViewRepositories {
            server_config,
            credential,
            offer,
            pre_authorized_code,
            access_token,
        },
    }
}
