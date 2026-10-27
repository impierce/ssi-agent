//! Integration test against a MongoDB event store running in Docker (`docker-tests` feature).
//!
//! MongoDB runs as a single-node replica set, as in `docs/introduction/quick-start.md`, since the event bus streams
//! live events from a change stream.

mod common;

use agent_shared::config::EventStoreType;
use testcontainers_modules::testcontainers::{
    core::{ExecCommand, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

#[tokio::test]
async fn application_state_survives_a_restart_on_mongodb() {
    // `testcontainers_modules::mongo::Mongo::repl_set` waits for log messages that MongoDB 8 no longer emits.
    let container = GenericImage::new("mongo", "8")
        .with_exposed_port(27017.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Waiting for connections"))
        .with_cmd(["--replSet", "rs0"])
        .start()
        .await
        .unwrap();
    container
        .exec(
            ExecCommand::new(["mongosh", "--quiet", "--eval", "rs.initiate()"])
                .with_container_ready_conditions(vec![WaitFor::message_on_stdout("Transition to primary complete")]),
        )
        .await
        .unwrap();

    // The replica set advertises its member under the container's internal hostname, so connect directly.
    let connection_string = format!(
        "mongodb://{}:{}/ssi-agent?directConnection=true",
        container.get_host().await.unwrap(),
        container.get_host_port_ipv4(27017).await.unwrap()
    );

    common::assert_state_survives_a_restart(EventStoreType::MongoDb, connection_string).await;
}
