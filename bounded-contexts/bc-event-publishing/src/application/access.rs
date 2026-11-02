use crate::{
    application::{command::EventPublishingCommand, query::EventPublishingQuery},
    domain::event_publisher::command::EventPublisherCommand,
};
use shared_kernel::authorization::CommandOperation;

pub const READ_PUBLISHERS_OPERATION: &str = "event_publishing.publishers.read";
pub const CREATE_PUBLISHER_OPERATION: &str = "event_publishing.publishers.create";
pub const UPDATE_PUBLISHER_OPERATION: &str = "event_publishing.publishers.update";
pub const ACTIVATE_PUBLISHER_OPERATION: &str = "event_publishing.publishers.activate";
pub const DEACTIVATE_PUBLISHER_OPERATION: &str = "event_publishing.publishers.deactivate";
pub const DELETE_PUBLISHER_OPERATION: &str = "event_publishing.publishers.delete";

#[must_use]
pub fn command_operation_name(command: &EventPublishingCommand) -> &'static str {
    match command {
        EventPublishingCommand::EventPublisher { command, .. } => event_publisher_command_operation(command),
    }
}

fn event_publisher_command_operation(command: &EventPublisherCommand) -> &'static str {
    command.operation_name()
}

#[must_use]
pub fn query_operation_name(query: &EventPublishingQuery) -> &'static str {
    match query {
        EventPublishingQuery::GetEventPublisherById { .. } | EventPublishingQuery::GetEventPublisherList => {
            READ_PUBLISHERS_OPERATION
        }
    }
}
