use crate::v0::issuance::error::{IntoPublicError, PublicError};
use crate::DOCUMENTATION_URL;
use agent_shared::handlers::{CommandHandlerError, QueryHandlerError};
use cqrs_es::{persist::PersistenceError, AggregateError};
use http_api_problem::ApiError;
use hyper::StatusCode;
use shared_kernel::authorization::AuthorizationError;

/// Wraps errors from the `cqrs_es` crate to be returned as API errors.
#[derive(Debug)]
pub enum ErrorWrapper<T: std::error::Error> {
    AggregateError(AggregateError<T>),
    CommandHandlerError(CommandHandlerError<T>),
    QueryHandlerError(QueryHandlerError),
    PersistenceError(PersistenceError),
}

// Helper function to construct type URLs for problem details.
pub fn type_url(path: &str) -> String {
    format!("{DOCUMENTATION_URL}problem-details/{path}")
}

impl<T: std::error::Error + IntoApiErrorExt> http_api_problem::IntoApiError for ErrorWrapper<T>
where
    T: Send + Sync + 'static,
{
    fn into_api_error(self) -> ApiError {
        match self {
            ErrorWrapper::AggregateError(error) => error.into_api_error(),
            ErrorWrapper::CommandHandlerError(error) => error.into_api_error(),
            ErrorWrapper::QueryHandlerError(error) => error.into_api_error(),
            ErrorWrapper::PersistenceError(error) => error.into_api_error(),
        }
    }
}

pub trait IntoApiErrorExt: std::error::Error {
    fn into_api_error(self) -> ApiError;
}

impl IntoApiErrorExt for ApiError {
    fn into_api_error(self) -> ApiError {
        self
    }
}

impl IntoApiErrorExt for AuthorizationError {
    fn into_api_error(self) -> ApiError {
        match self {
            AuthorizationError::Unauthorized => ApiError::builder(StatusCode::UNAUTHORIZED)
                .title("Unauthorized")
                .type_url(type_url("authorization#unauthorized"))
                .message("Authentication is required")
                .finish(),
            AuthorizationError::Forbidden => ApiError::builder(StatusCode::FORBIDDEN)
                .title("Forbidden")
                .type_url(type_url("authorization#forbidden"))
                .message("The request is not authorized")
                .finish(),
        }
    }
}

impl<T: std::error::Error + IntoPublicError> From<ErrorWrapper<T>> for PublicError {
    fn from(err: ErrorWrapper<T>) -> Self {
        match err {
            ErrorWrapper::AggregateError(error) => PublicError::from(error),
            ErrorWrapper::CommandHandlerError(error) => match error {
                CommandHandlerError::Authorization(_) => PublicError::InternalServerError,
                CommandHandlerError::Aggregate(error) => PublicError::from(error),
            },
            ErrorWrapper::QueryHandlerError(error) => match error {
                QueryHandlerError::Authorization(_) | QueryHandlerError::Persistence(_) => {
                    PublicError::InternalServerError
                }
            },
            ErrorWrapper::PersistenceError(error) => PublicError::from(error),
        }
    }
}

impl<T: std::error::Error + IntoPublicError> From<AggregateError<T>> for PublicError {
    fn from(err: AggregateError<T>) -> Self {
        match err {
            AggregateError::UserError(error) => error.into_public_error(),
            _ => PublicError::InternalServerError,
        }
    }
}

impl<T: IntoApiErrorExt> IntoApiErrorExt for AggregateError<T>
where
    T: Send + Sync + 'static,
{
    fn into_api_error(self) -> ApiError {
        match self {
            AggregateError::UserError(error) => error.into_api_error(),
            AggregateError::AggregateConflict => ApiError::builder(StatusCode::SERVICE_UNAVAILABLE)
            .title("Aggregate Conflict")
            .type_url(type_url("persistence#aggregate-conflict"))
            .message("The server is currently unable to handle the request due to temporary overloading or maintenance. Please try again later.")
            .finish(),
        AggregateError::DatabaseConnectionError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
            .title("Database Connection Error")
            .type_url(type_url("persistence#database-connection-error"))
            .source_in_a_box(error)
            .finish(),
        AggregateError::DeserializationError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
            .title("Deserialization Error")
            .type_url(type_url("persistence#deserialization-error"))
            .message("The system failed to deserialize events from the event store due to a schema mismatch. Data migration is not supported; therefore, the only resolution is to reset the event store by wiping the existing data.")
            .source_in_a_box(error)
            .finish(),
        AggregateError::UnexpectedError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
            .title("Unexpected Error")
            .type_url(type_url("unexpected#unexpected-error"))
            .source_in_a_box(error)
                .finish(),
        }
    }
}

impl<T: IntoApiErrorExt> IntoApiErrorExt for CommandHandlerError<T>
where
    T: Send + Sync + 'static,
{
    fn into_api_error(self) -> ApiError {
        match self {
            CommandHandlerError::Authorization(error) => error.into_api_error(),
            CommandHandlerError::Aggregate(error) => error.into_api_error(),
        }
    }
}

impl IntoApiErrorExt for QueryHandlerError {
    fn into_api_error(self) -> ApiError {
        match self {
            QueryHandlerError::Authorization(error) => error.into_api_error(),
            QueryHandlerError::Persistence(error) => error.into_api_error(),
        }
    }
}

impl IntoApiErrorExt for PersistenceError {
    fn into_api_error(self) -> ApiError {
        AggregateError::<ApiError>::from(self).into_api_error()
    }
}

impl From<PersistenceError> for PublicError {
    fn from(_err: PersistenceError) -> Self {
        PublicError::InternalServerError
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use axum::response::Response;
    use http_api_problem::IntoApiError;
    use serde_json::json;
    use std::{error::Error, fmt};

    pub async fn into_json_value(response: Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn persistence_errors_successfully_convert_to_problem_details() {
        assert_eq!(
            into_json_value(
                PersistenceError::OptimisticLockError
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/persistence#aggregate-conflict"),
                "title": "Aggregate Conflict",
                "status": 503,
                "detail": "The server is currently unable to handle the request due to temporary overloading or maintenance. Please try again later."
            }),
        );

        assert_eq!(
            into_json_value(
                PersistenceError::ConnectionError("A problem occurred while connecting to the database".into())
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/persistence#database-connection-error"),
                "title": "Database Connection Error",
                "status": 500,
                "detail": "A problem occurred while connecting to the database"
            }),
        );

        assert_eq!(
            into_json_value(
                PersistenceError::DeserializationError("A problem occurred while deserializing the data".into())
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/persistence#deserialization-error"),
                "title": "Deserialization Error",
                "status": 500,
                "detail": "The system failed to deserialize events from the event store due to a schema mismatch. Data migration is not supported; therefore, the only resolution is to reset the event store by wiping the existing data."
            }),
        );

        assert_eq!(
            into_json_value(
                PersistenceError::UnknownError("An unexpected error occurred".into())
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"),
                "title": "Unexpected Error",
                "status": 500,
                "detail": "An unexpected error occurred"
            }),
        );
    }

    #[tokio::test]
    async fn command_handler_errors_successfully_convert_to_problem_details() {
        assert_eq!(
            into_json_value(
                ErrorWrapper::<ApiError>::CommandHandlerError(CommandHandlerError::Authorization(
                    AuthorizationError::Forbidden
                ))
                .into_api_error()
                .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/authorization#forbidden"),
                "title": "Forbidden",
                "status": 403,
                "detail": "The request is not authorized"
            }),
        );

        assert_eq!(
            into_json_value(
                ErrorWrapper::CommandHandlerError(CommandHandlerError::Aggregate(AggregateError::UserError(
                    ApiError::builder(StatusCode::BAD_REQUEST)
                        .title("Invalid Command")
                        .type_url(type_url("test#invalid-command"))
                        .message("The command is invalid")
                        .finish(),
                )))
                .into_api_error()
                .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/test#invalid-command"),
                "title": "Invalid Command",
                "status": 400,
                "detail": "The command is invalid"
            }),
        );
    }

    #[test]
    fn command_handler_errors_successfully_convert_to_public_errors() {
        assert!(matches!(
            PublicError::from(ErrorWrapper::<TestPublicError>::CommandHandlerError(
                CommandHandlerError::Authorization(AuthorizationError::Forbidden)
            )),
            PublicError::InternalServerError
        ));

        assert!(matches!(
            PublicError::from(ErrorWrapper::CommandHandlerError(CommandHandlerError::Aggregate(
                AggregateError::UserError(TestPublicError)
            ))),
            PublicError::NotFoundError
        ));
    }

    #[derive(Debug)]
    struct TestPublicError;

    impl fmt::Display for TestPublicError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("test public error")
        }
    }

    impl Error for TestPublicError {}

    impl IntoPublicError for TestPublicError {
        fn into_public_error(self) -> PublicError {
            PublicError::NotFoundError
        }
    }
}
