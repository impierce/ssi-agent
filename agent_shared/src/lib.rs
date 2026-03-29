pub mod application_state;
pub mod config;
pub mod credential_status_checker;
pub mod custom_queries;
pub mod error;
pub mod generic_query;
pub mod handlers;
pub mod profile;
pub mod serde_json_value_ext;
pub mod url_utils;

pub use ::config::ConfigError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use identity_core::convert::{FromJson as _, ToJson as _};
use identity_iota::verification::jws::JwsAlgorithm;
use jsonwebtoken::{jwk::Jwk as JsonWebTokenJwk, DecodingKey};
use rand::Rng;
pub use url_utils::UrlAppendHelpers;

pub fn generate_random_string() -> String {
    let mut rng = rand::rng();

    // Generate 32 random bytes (256 bits)
    let random_bytes: [u8; 32] = rng.random();

    // Convert the random bytes to a hexadecimal string
    let random_string: String = random_bytes.iter().fold(String::new(), |mut acc, byte| {
        acc.push_str(&format!("{byte:02x}"));
        acc
    });

    random_string
}

/// Helper function that converts `jsonwebtoken::Algorithm` to `JwsAlgorithm`.
pub fn from_jsonwebtoken_algorithm_to_jwsalgorithm(algorithm: &jsonwebtoken::Algorithm) -> JwsAlgorithm {
    match algorithm {
        jsonwebtoken::Algorithm::HS256 => JwsAlgorithm::HS256,
        jsonwebtoken::Algorithm::HS384 => JwsAlgorithm::HS384,
        jsonwebtoken::Algorithm::HS512 => JwsAlgorithm::HS512,
        jsonwebtoken::Algorithm::ES256 => JwsAlgorithm::ES256,
        jsonwebtoken::Algorithm::ES384 => JwsAlgorithm::ES384,
        jsonwebtoken::Algorithm::RS256 => JwsAlgorithm::RS256,
        jsonwebtoken::Algorithm::RS384 => JwsAlgorithm::RS384,
        jsonwebtoken::Algorithm::RS512 => JwsAlgorithm::RS512,
        jsonwebtoken::Algorithm::PS256 => JwsAlgorithm::PS256,
        jsonwebtoken::Algorithm::PS384 => JwsAlgorithm::PS384,
        jsonwebtoken::Algorithm::PS512 => JwsAlgorithm::PS512,
        jsonwebtoken::Algorithm::EdDSA => JwsAlgorithm::EdDSA,
    }
}

/// Get the claims from a JWT without performing validation.
pub fn get_unverified_jwt_claims(jwt: &serde_json::Value) -> Option<serde_json::Value> {
    jwt.as_str()
        .and_then(|string| string.splitn(3, '.').collect::<Vec<&str>>().get(1).cloned())
        .and_then(|payload| {
            URL_SAFE_NO_PAD
                .decode(payload)
                .ok()
                .and_then(|payload_bytes| serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok())
        })
}

/// Convert the `IotaIdentityJwk` first into a `JsonWebTokenJwk` and then into a `DecodingKey`.
pub fn convert_iota_jwk_to_decoding_key(public_key: &identity_jose::jwk::Jwk) -> Option<DecodingKey> {
    let decoding_key = public_key
        .to_json()
        .ok()
        .and_then(|public_key| JsonWebTokenJwk::from_json(&public_key).ok())
        .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok());

    decoding_key
}
