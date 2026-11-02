use crate::error::{type_url, IntoApiErrorExt};
use bc_event_publishing::{
    application::{command::EventPublishingCommandError, query::EventPublishingQueryError},
    domain::event_publisher::error::EventPublisherError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for EventPublisherError {
    fn into_api_error(self) -> ApiError {
        let message = self.to_string();
        match self {
            EventPublisherError::NotFound(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Event Publisher Not Found")
                .type_url(type_url("events#event-publisher-not-found"))
                .message(message)
                .finish(),
            EventPublisherError::AlreadyExists(_) => ApiError::builder(StatusCode::CONFLICT)
                .title("Event Publisher Already Exists")
                .type_url(type_url("events#event-publisher-already-exists"))
                .message(message)
                .finish(),
            EventPublisherError::EmptyName => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Event Publisher Name")
                .type_url(type_url("events#invalid-event-publisher-name"))
                .message(message)
                .finish(),
            EventPublisherError::InvalidEvents(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Event Types")
                .type_url(type_url("events#invalid-event-types"))
                .message(message)
                .finish(),
            EventPublisherError::InvalidConfiguration(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Event Publisher Configuration")
                .type_url(type_url("events#invalid-event-publisher-configuration"))
                .message(message)
                .finish(),
        }
    }
}

impl IntoApiErrorExt for EventPublishingCommandError {
    fn into_api_error(self) -> ApiError {
        match self {
            EventPublishingCommandError::EventPublisherError(err) => err.into_api_error(),
        }
    }
}

impl IntoApiErrorExt for EventPublishingQueryError {
    fn into_api_error(self) -> ApiError {
        match self {
            EventPublishingQueryError::PersistenceError(err) => err.into_api_error(),
            EventPublishingQueryError::NotFound(id) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Event Publisher Not Found")
                .type_url(type_url("events#event-publisher-not-found"))
                .message(format!("Event publisher `{id}` not found"))
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::tests::assert_problems;
    use bc_event_publishing::domain::event_publisher::{config::InvalidPublisherConfig, services::InvalidEventTypes};

    #[test]
    fn event_publisher_errors_map_to_expected_problem_details() {
        assert_problems([
            (
                EventPublisherError::NotFound("pub-1".to_string()),
                StatusCode::NOT_FOUND,
                Some("events#event-publisher-not-found"),
            ),
            (
                EventPublisherError::AlreadyExists("pub-1".to_string()),
                StatusCode::CONFLICT,
                Some("events#event-publisher-already-exists"),
            ),
            (
                EventPublisherError::EmptyName,
                StatusCode::BAD_REQUEST,
                Some("events#invalid-event-publisher-name"),
            ),
            (
                EventPublisherError::InvalidEvents(InvalidEventTypes::EmptyEvents),
                StatusCode::BAD_REQUEST,
                Some("events#invalid-event-types"),
            ),
            (
                EventPublisherError::InvalidConfiguration(InvalidPublisherConfig::InvalidTargetUrl {
                    url: "bad".to_string(),
                    reason: "invalid".to_string(),
                }),
                StatusCode::BAD_REQUEST,
                Some("events#invalid-event-publisher-configuration"),
            ),
        ]);
    }
}
