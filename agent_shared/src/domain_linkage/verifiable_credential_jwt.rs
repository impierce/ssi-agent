use is_empty::IsEmpty;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::error::SharedError;

/// Set of IANA registered claims by the Internet Engineering Task Force (IETF) in
/// [RFC 7519](https://tools.ietf.org/html/rfc7519#section-4.1).
#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, IsEmpty)]
pub struct RFC7519Claims {
    pub iss: Option<String>,
    pub sub: Option<String>,
    pub aud: Option<String>,
    pub exp: Option<i64>,
    pub nbf: Option<i64>,
    pub iat: Option<i64>,
    pub jti: Option<String>,
}

// Macro that generates a builder function for a field.
#[macro_export]
macro_rules! builder_fn {
    ($name:ident, $ty:ty) => {
        #[allow(clippy::should_implement_trait)]
        pub fn $name(mut self, value: impl Into<$ty>) -> Self {
            self.$name.replace(value.into());
            self
        }
    };
    ($field:ident, $name:ident, $ty:ty) => {
        #[allow(clippy::should_implement_trait)]
        pub fn $name(mut self, value: impl Into<$ty>) -> Self {
            self.$field.$name.replace(value.into());
            self
        }
    };
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct VerifiableCredentialJwt {
    #[serde(flatten)]
    pub rfc7519_claims: RFC7519Claims,
    #[serde(rename = "vc")]
    pub verifiable_credential: serde_json::Value,
}

impl VerifiableCredentialJwt {
    pub fn builder() -> VerifiableCredentialJwtBuilder {
        VerifiableCredentialJwtBuilder::new()
    }
}

#[derive(Default)]
pub struct VerifiableCredentialJwtBuilder {
    rfc7519_claims: RFC7519Claims,
    verifiable_credential: Option<serde_json::Value>,
}

impl VerifiableCredentialJwtBuilder {
    pub fn new() -> Self {
        VerifiableCredentialJwtBuilder::default()
    }

    pub fn build(self) -> Result<VerifiableCredentialJwt, SharedError> {
        Ok(VerifiableCredentialJwt {
            rfc7519_claims: self.rfc7519_claims,
            verifiable_credential: self
                .verifiable_credential
                .ok_or(SharedError::Generic("`verifiable_credential` is required".to_string()))?,
        })
    }

    builder_fn!(rfc7519_claims, iss, String);
    builder_fn!(rfc7519_claims, sub, String);
    builder_fn!(rfc7519_claims, aud, String);
    builder_fn!(rfc7519_claims, exp, i64);
    builder_fn!(rfc7519_claims, nbf, i64);
    builder_fn!(rfc7519_claims, iat, i64);
    builder_fn!(rfc7519_claims, jti, String);
    builder_fn!(verifiable_credential, serde_json::Value);
}
