use identity_kit::{sd_jwt_vc::SdJwt, vc::Jwt};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;

/// A credential in the compact serialization an issuer sent it in: a JWT or an SD-JWT.
///
/// It serializes as that string, so events that stored a credential as a plain string read
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SignedCredential {
    Jwt(Jwt),
    SdJwt(SdJwt),
}

#[derive(Error, Debug)]
#[error("Credential is neither a JWT nor an SD-JWT")]
pub struct UnsupportedCredentialFormat;

impl FromStr for SignedCredential {
    type Err = UnsupportedCredentialFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(jwt) = s.parse() {
            return Ok(Self::Jwt(jwt));
        }
        s.parse().map(Self::SdJwt).map_err(|_| UnsupportedCredentialFormat)
    }
}

impl fmt::Display for SignedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jwt(jwt) => jwt.fmt(f),
            Self::SdJwt(sd_jwt) => sd_jwt.fmt(f),
        }
    }
}

impl From<Jwt> for SignedCredential {
    fn from(jwt: Jwt) -> Self {
        Self::Jwt(jwt)
    }
}

/// An SD-JWT without disclosures, whose signature is not checked anywhere it is used.
#[cfg(test)]
pub(crate) const SD_JWT: &str = concat!(
    "eyJhbGciOiJFUzI1NiIsInR5cCI6ImRjK3NkLWp3dCJ9.",
    "eyJfc2QiOltdLCJfc2RfYWxnIjoic2hhLTI1NiIsImlzcyI6Imh0dHBzOi8vaXNzdWVyLmV4YW1wbGUiLCJ2Y3QiOiJodHRwczovL3ZjdC5leGFtcGxlIn0.",
    "c2lnbmF0dXJl~"
);

#[cfg(test)]
mod tests {
    use super::*;
    use agent_issuance::credential::aggregate::test_utils::JWT_VC_JSON_OBV3_JWT;

    #[test]
    fn reads_and_writes_a_jwt_as_its_string() {
        let credential: SignedCredential = serde_json::from_value(serde_json::json!(JWT_VC_JSON_OBV3_JWT)).unwrap();

        assert!(matches!(credential, SignedCredential::Jwt(_)));
        assert_eq!(serde_json::to_value(&credential).unwrap(), JWT_VC_JSON_OBV3_JWT);
        assert_eq!(JWT_VC_JSON_OBV3_JWT.parse::<SignedCredential>().unwrap(), credential);
    }

    #[test]
    fn reads_and_writes_an_sd_jwt_as_its_string() {
        let credential: SignedCredential = serde_json::from_value(serde_json::json!(SD_JWT)).unwrap();

        assert!(matches!(credential, SignedCredential::SdJwt(_)));
        assert_eq!(serde_json::to_value(&credential).unwrap(), SD_JWT);
        assert_eq!(SD_JWT.parse::<SignedCredential>().unwrap(), credential);
    }

    #[test]
    fn refuses_other_strings() {
        assert!("not a credential".parse::<SignedCredential>().is_err());
        assert!(serde_json::from_value::<SignedCredential>(serde_json::json!("not a credential")).is_err());
    }
}
