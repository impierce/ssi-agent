use identity_kit::{
    jose::DefaultVerifier,
    sd_jwt_vc::{SdJwt, SdJwtCredentialV2, SdJwtCredentialValidator, SdJwtValidationOptions},
    vc::{
        validation::{KeyResolver, StatusPolicy},
        Jwt,
    },
};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;

/// A credential in the compact serialization an issuer sent it in: a JWT or an SD-JWT.
// It serializes as that string, so events that stored a credential as a plain string read
// unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
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

/// Why an SD-JWT credential is not one the holder keeps.
#[derive(Error, Debug)]
pub enum SdJwtCredentialError {
    /// Not a VCDM 2.0 credential secured with SD-JWT (`vc+sd-jwt`), e.g. an SD-JWT VC (`dc+sd-jwt`).
    #[error("not a `vc+sd-jwt` credential: {0}")]
    UnsupportedFormat(String),
    /// The issuer signature or the validity period does not hold.
    #[error("{0}")]
    Invalid(String),
}

/// Verifies `sd_jwt` as a VCDM 2.0 credential secured with SD-JWT (`vc+sd-jwt`): the signature of the issuer it
/// names and its validity period, with the disclosures it carries applied. Status is not checked here.
pub async fn verify_sd_jwt_credential(
    sd_jwt: &SdJwt,
    resolver: &(impl KeyResolver + ?Sized),
) -> Result<SdJwtCredentialV2, SdJwtCredentialError> {
    let credential = SdJwtCredentialV2::from_sd_jwt(sd_jwt.clone())
        .map_err(|error| SdJwtCredentialError::UnsupportedFormat(error.to_string()))?;
    let options = SdJwtValidationOptions::new().with_status(StatusPolicy::SkipAll);
    SdJwtCredentialValidator::new(resolver, DefaultVerifier)
        .validate_credential_v2(credential, &options)
        .await
        .map(|validated| validated.credential)
        .map_err(|error| SdJwtCredentialError::Invalid(error.to_string()))
}

/// An SD-JWT without disclosures, whose signature is not checked anywhere it is used.
#[cfg(test)]
pub(crate) const SD_JWT: &str = concat!(
    "eyJhbGciOiJFUzI1NiIsInR5cCI6ImRjK3NkLWp3dCJ9.",
    "eyJfc2QiOltdLCJfc2RfYWxnIjoic2hhLTI1NiIsImlzcyI6Imh0dHBzOi8vaXNzdWVyLmV4YW1wbGUiLCJ2Y3QiOiJodHRwczovL3ZjdC5leGFtcGxlIn0.",
    "c2lnbmF0dXJl~"
);

/// A `vc+sd-jwt` credential issued by the `did:key` of `holder` about itself, valid until `valid_until`, with its
/// `name` selectively disclosable and disclosed.
#[cfg(test)]
pub(crate) async fn issue_vc_sd_jwt(
    holder: &agent_secret_manager::subject::Subject,
    valid_until: chrono::DateTime<chrono::Utc>,
) -> SdJwt {
    use chrono::TimeZone as _;
    use identity_kit::{
        did::Did,
        sd_jwt_vc::SdJwtCredentialV2Builder,
        vc::{CredentialV2, Subject},
    };
    use oid4vc_core::{Sign as _, Subject as _};

    let algorithm = jsonwebtoken::Algorithm::EdDSA;
    let did: Did = holder.identifier("did:key", algorithm).await.unwrap().parse().unwrap();
    let kid = holder.key_id("did:key", algorithm).await.unwrap();
    let credential = CredentialV2::new(did.clone(), Subject::new(did).with_property("name", "Example LLC"))
        .with_type("OrganizationCredential")
        .with_valid_from(chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap())
        .with_valid_until(valid_until);
    SdJwtCredentialV2Builder::new(credential)
        .with_kid(kid)
        .conceal(["credentialSubject", "name"])
        .sign(&holder.issuer_signer(algorithm).await.unwrap())
        .await
        .unwrap()
        .into_sd_jwt()
}

/// `sd_jwt` with the signature of its issuer-signed JWT replaced.
#[cfg(test)]
pub(crate) fn with_forged_signature(sd_jwt: &SdJwt) -> SdJwt {
    let serialized = sd_jwt.to_string();
    let (jwt, disclosures) = serialized.split_once('~').unwrap();
    let (signed, _signature) = jwt.rsplit_once('.').unwrap();
    format!("{signed}.c2lnbmF0dXJl~{disclosures}").parse().unwrap()
}

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
