pub mod application_state;
pub mod config;
pub mod domain_linkage;
pub mod error;
pub mod generic_query;
pub mod handlers;
pub mod url_utils;

pub use ::config::ConfigError;
use identity_iota::verification::jws::JwsAlgorithm;
use rand::Rng;
pub use url_utils::UrlAppendHelpers;

pub fn generate_random_string() -> String {
    let mut rng = rand::thread_rng();

    // Generate 32 random bytes (256 bits)
    let random_bytes: [u8; 32] = rng.gen();

    // Convert the random bytes to a hexadecimal string
    let random_string: String = random_bytes.iter().fold(String::new(), |mut acc, byte| {
        acc.push_str(&format!("{:02x}", byte));
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
