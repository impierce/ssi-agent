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

/// A helper module for deserializing fields that can be present with a value,
/// present and explicitly `null`, or not present at all.
pub(crate) mod serde_explicit_null {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Deserializes a field that is present, wrapping it in `Some`.
    /// This allows us to distinguish between `{"key": null}` (`Some(None)`)
    /// and `{}` (`None`).
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        // Deserialize the inner value (which could be `T` or `null`) into an `Option<T>`.
        let inner = Option::<T>::deserialize(deserializer)?;
        // Wrap the result in `Some` to indicate the key was present in the JSON.
        Ok(Some(inner))
    }

    /// Serializes the inner `Option<T>` of `Option<Option<T>>`.
    /// This should be used with `#[serde(skip_serializing_if = "Option::is_none")]`
    /// on the field to omit it when the outer `Option` is `None`.
    pub fn serialize<S, T>(value: &Option<Option<T>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        match value {
            Some(inner) => inner.serialize(serializer),
            // This branch is technically unreachable if `skip_serializing_if` is used,
            // but we handle it for completeness.
            None => serializer.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::serde_explicit_null;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Deserialize, Serialize, Debug, PartialEq)]
    struct TestStruct {
        #[serde(default, with = "serde_explicit_null")]
        key: Option<Option<String>>,
    }

    #[test]
    fn test_deserialize_explicit_null() {
        let json_with_value = json!({ "key": "value" });
        let json_with_null = json!({ "key": null });
        let json_without_key = json!({});

        let result_with_value: TestStruct = serde_json::from_value(json_with_value).unwrap();
        let result_with_null: TestStruct = serde_json::from_value(json_with_null).unwrap();
        let result_without_key: TestStruct = serde_json::from_value(json_without_key).unwrap();

        assert_eq!(result_with_value.key, Some(Some("value".to_string())));
        assert_eq!(result_with_null.key, Some(None));
        assert_eq!(result_without_key.key, None);
    }
}
