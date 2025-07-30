use axum::{
    extract::{rejection::RawFormRejection, FromRequest, FromRequestParts, RawForm, Request},
    response::{IntoResponse, Response},
    RequestExt as _,
};
use http::{request::Parts, StatusCode};
use serde::de::DeserializeOwned;

/// An Axum extractor for `application/x-www-form-urlencoded` data with a special deserialization strategy.
///
/// This extractor is similar to `axum::Form`, but it is designed to handle a specific
/// encoding style where complex fields (like JSON objects or arrays) are first
/// stringified into a single string and *then* form-encoded. This is a common requirement
/// in specifications like OpenID for Verifiable Credentials (OID4VCI).
///
/// # Deserialization Strategy
///
/// Instead of `serde_urlencoded`'s default behavior which flattens nested objects,
/// `StringifiedForm` uses a custom deserializer (`oid4vci::from_form_urlencoded_string`).
/// This function first decodes the form into a map of string keys and string values.
/// It then iterates through the map, attempting to parse each string value as JSON.
/// If successful, the parsed JSON value is used; otherwise, it falls back to treating
/// the value as a simple string. Finally, the resulting JSON-like object is
/// deserialized into the target type `T`.
///
/// # Example
///
/// Consider a form submission for an `AuthorizationRequest` where `authorization_details`
/// is an array of objects.
///
/// A standard `axum::Form` would expect the data to be flattened:
/// `authorization_details[0][type]=openid_credential&...`
///
/// `StringifiedForm` expects the complex field to be stringified first:
/// `authorization_details=%5B%7B%22type%22%3A%22openid_credential%22%7D%5D`
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct StringifiedForm<T>(pub T);

impl<T, S> FromRequest<S> for StringifiedForm<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let is_get_or_head = req.method() == http::Method::GET || req.method() == http::Method::HEAD;

        match req.extract().await {
            Ok(RawForm(bytes)) => {
                let value = oid4vci::from_form_urlencoded_string(
                    std::str::from_utf8(&bytes)
                        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to deserialize form").into_response())?,
                )
                .map_err(|_| -> Response {
                    if is_get_or_head {
                        (StatusCode::BAD_REQUEST, "Failed to deserialize form").into_response()
                    } else {
                        (StatusCode::UNPROCESSABLE_ENTITY, "Failed to deserialize form body").into_response()
                    }
                })?;
                Ok(StringifiedForm(value))
            }
            Err(RawFormRejection::BytesRejection(_)) => {
                Err((StatusCode::BAD_REQUEST, "Failed to deserialize form").into_response())
            }
            Err(RawFormRejection::InvalidFormContentType(_)) => {
                Err((StatusCode::BAD_REQUEST, "Invalid form content type").into_response())
            }
            Err(_) => Err((StatusCode::BAD_REQUEST, "Failed to deserialize form").into_response()),
        }
    }
}

/// An Axum extractor for query strings with a special deserialization strategy.
///
/// This extractor is similar to `axum::extract::Query`, but it is designed to handle
/// a specific encoding style where complex fields (like JSON objects or arrays) are first
/// stringified into a single string and *then* URL-encoded as a query parameter. This
/// is a common requirement in specifications like OpenID for Verifiable Credentials (OID4VCI).
///
/// # Deserialization Strategy
///
/// Instead of `serde_urlencoded`'s default behavior which flattens nested objects,
/// `StringifiedQuery` uses a custom deserializer (`oid4vci::from_form_urlencoded_string`).
/// This function first decodes the query string into a map of string keys and string values.
/// It then iterates through the map, attempting to parse each string value as JSON.
/// If successful, the parsed JSON value is used; otherwise, it falls back to treating
/// the value as a simple string. Finally, the resulting JSON-like object is
/// deserialized into the target type `T`.
///
/// # Example
///
/// Consider a request to an endpoint with an `AuthorizationRequest` in the query string,
/// where `authorization_details` is an array of objects.
///
/// A standard `axum::extract::Query` would expect the data to be flattened:
/// `.../authorize?authorization_details[0][type]=openid_credential&...`
///
/// `StringifiedQuery` expects the complex field to be stringified first:
/// `.../authorize?authorization_details=%5B%7B%22type%22%3A%22openid_credential%22%7D%5D`
#[derive(Debug, Clone, Copy, Default)]
pub struct StringifiedQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for StringifiedQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();
        let params = oid4vci::from_form_urlencoded_string(&query)
            .map_err(|_| (StatusCode::BAD_REQUEST, format!("Failed to deserialize query string")).into_response())?;

        Ok(StringifiedQuery(params))
    }
}
