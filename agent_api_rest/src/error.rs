use cqrs_es::{persist::PersistenceError, AggregateError};
use http_api_problem::ApiError;
use hyper::StatusCode;

/// Wraps errors from the `cqrs_es` crate to be returned as API errors.
#[derive(Debug)]
pub enum ErrorWrapper<T: std::error::Error> {
    AggregateError(AggregateError<T>),
    PersistenceError(PersistenceError),
}

impl<T: std::error::Error + crate::error::IntoApiErrorExt> http_api_problem::IntoApiError for ErrorWrapper<T> {
    fn into_api_error(self) -> ApiError {
        match self {
            ErrorWrapper::AggregateError(error) => error.into_api_error(),
            ErrorWrapper::PersistenceError(error) => error.into_api_error(),
        }
    }
}

pub trait IntoApiErrorExt: std::error::Error {
    fn into_api_error(self) -> ApiError;
}

impl<T: IntoApiErrorExt> IntoApiErrorExt for AggregateError<T> {
    fn into_api_error(self) -> ApiError {
        match self {
            AggregateError::UserError(error) => error.into_api_error(),
            AggregateError::AggregateConflict => ApiError::builder(StatusCode::CONFLICT)
                .title("Aggregate Conflict")
                .message("A command has been rejected due to a conflict with another command on the same aggregate instance.")
                .finish(),
            AggregateError::DatabaseConnectionError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Database Connection Error")
                .message(error.to_string())
                .finish(),
            AggregateError::DeserializationError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Deserialization Error")
                .message(error.to_string())
                .finish(),
            AggregateError::UnexpectedError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .message(error.to_string())
                .finish(),
        }
    }
}

impl IntoApiErrorExt for PersistenceError {
    fn into_api_error(self) -> ApiError {
        match self {
            PersistenceError::OptimisticLockError => ApiError::builder(StatusCode::CONFLICT)
                .title("Optimistic Lock Error")
                .message("An optimistic lock error occurred while committing an aggregate.")
                .finish(),
            PersistenceError::ConnectionError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Connection Error")
                .message(error.to_string())
                .source_in_a_box(error)
                .finish(),
            PersistenceError::DeserializationError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Deserialization Error")
                .message(error.to_string())
                .source_in_a_box(error)
                .finish(),
            PersistenceError::UnknownError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unknown Error")
                .message(error.to_string())
                .source_in_a_box(error)
                .finish(),
        }
    }
}
