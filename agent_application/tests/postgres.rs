//! Integration test against a PostgreSQL event store running in Docker (`docker-tests` feature).

mod common;

use agent_shared::config::EventStoreType;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{runners::AsyncRunner, ImageExt},
};

#[tokio::test]
async fn application_state_survives_a_restart_on_postgres() {
    let container = Postgres::default()
        .with_init_sql(include_str!("../docker/db/init.sql").as_bytes().to_vec())
        .with_tag("17-alpine")
        .start()
        .await
        .unwrap();
    let connection_string = format!(
        "postgresql://postgres:postgres@{}:{}/postgres",
        container.get_host().await.unwrap(),
        container.get_host_port_ipv4(5432).await.unwrap()
    );

    common::assert_state_survives_a_restart(EventStoreType::Postgres, connection_string).await;
}
