use std::convert::Infallible;

use axum::extract::FromRequestParts;
use shared_kernel::authorization::Actor;

/// Optional actor extracted from request extensions.
///
/// The actor extraction middleware stores an `Actor` in request extensions when
/// one is available. Handlers can use this extractor to receive that value as an
/// `Option<Actor>` without depending on Axum's `Extension` wrapper directly.
pub struct RequestActor(pub Option<Actor>);

impl<S> FromRequestParts<S> for RequestActor
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut http::request::Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<Actor>().cloned()))
    }
}
