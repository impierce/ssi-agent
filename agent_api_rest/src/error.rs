use crate::DOCUMENTATION_URL;
use cqrs_es::{persist::PersistenceError, AggregateError};
use http_api_problem::ApiError;
use hyper::StatusCode;

/// Wraps errors from the `cqrs_es` crate to be returned as API errors.
#[derive(Debug)]
pub enum ErrorWrapper<T: std::error::Error> {
    AggregateError(AggregateError<T>),
    PersistenceError(PersistenceError),
}

impl<T: std::error::Error + crate::error::IntoApiErrorExt> http_api_problem::IntoApiError for ErrorWrapper<T>
where
    T: Send + Sync + 'static,
{
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

impl IntoApiErrorExt for ApiError {
    fn into_api_error(self) -> ApiError {
        self
    }
}

impl IntoApiErrorExt for PersistenceError {
    fn into_api_error(self) -> ApiError {
        AggregateError::<ApiError>::from(self).into_api_error()
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
                .type_url(format!("{DOCUMENTATION_URL}problem-details/persistence#aggregate-conflict"))
                .message("The server is currently unable to handle the request due to temporary overloading or maintenance. Please try again later.")
                .finish(),
            AggregateError::DatabaseConnectionError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Database Connection Error")
                .type_url(format!("{DOCUMENTATION_URL}problem-details/persistence#database-connection-error"))
                .source_in_a_box(error)
                .finish(),
            AggregateError::DeserializationError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Deserialization Error")
                .type_url(format!("{DOCUMENTATION_URL}problem-details/persistence#deserialization-error"))
                .message("The system failed to deserialize events from the event store due to a schema mismatch. Data migration is not supported; therefore, the only resolution is to reset the event store by wiping the existing data.")
                .source_in_a_box(error)
                .finish(),
            AggregateError::UnexpectedError(error) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .type_url(format!("{DOCUMENTATION_URL}problem-details/persistence#unexpected-error"))
                .title("Unexpected Error")
                .source_in_a_box(error)
                .finish(),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use axum::response::Response;
    use serde_json::json;

    async fn into_json_value(response: Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn test() {
        let api_error: ApiError = PersistenceError::OptimisticLockError.into_api_error();

        let test = into_json_value(api_error.into_axum_response()).await;

        assert_eq!(
            json!({
                "type": "https://httpstatuses.com/503",
                "title": "Service Unavailable",
                "status": 503,
                "detail": "The server is currently unable to handle the request due to temporary overloading or maintenance. Please try again later."
            }),
            test
        );
    }
}

// # Errors

// - API Errors (can never panic)

//   - UniCore API Errors
//     - Persistence Layer Errors
//     - Client Errors
//     - Third-Party Errors
//   - Specification Errors
//     - Persistence Layer Errors
//     - OpenID4VC API Errors
//     - Domain Linkage Errors
//     - Linked Verifiable Presentation Errors

// - Configuration Errors (may panic)
//   - Persistence Layer Errors
