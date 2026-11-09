pub mod application_state;
pub mod config;
pub mod credential_status_checker;
pub mod custom_queries;
pub mod error;
pub mod generic_query;
pub mod handlers;
pub mod profile;
pub mod sensitive_logging;
pub mod serde_json_value_ext;
pub mod signed_credential_format;
pub mod url_utils;

pub use ::config::ConfigError;
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
