use axum::extract::{
    rejection::{FormRejection, QueryRejection},
    FromRequest, FromRequestParts, Request,
};
use http::request::Parts;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, Copy, Default)]
pub struct StringifiedForm<T>(pub T);

// FIXME
impl<T, S> FromRequest<S> for StringifiedForm<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = FormRejection;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        // Check for the correct content type
        let content_type = req
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());

        // if let Some(content_type) = content_type {
        //     if content_type != mime::APPLICATION_WWW_FORM_URLENCODED.as_ref() {
        //         return Err(FormRejection::InvalidContentType(content_type.to_string()));
        //     }
        // } else {
        //     return Err(FormRejection::MissingContentType);
        // }

        // Read the full request body
        let bytes = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap();

        // Use the custom deserialization logic
        let value = oid4vci::from_form_urlencoded_string(std::str::from_utf8(&bytes).unwrap()).unwrap();

        Ok(StringifiedForm(value))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StringifiedQuery<T>(pub T);

// FIXME
impl<T, S> FromRequestParts<S> for StringifiedQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = QueryRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();
        let params = oid4vci::from_form_urlencoded_string(&query).unwrap();

        Ok(StringifiedQuery(params))
    }
}
