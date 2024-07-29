pub mod application_state;
pub mod config;
pub mod domain_linkage;
pub mod error;
pub mod generic_query;
pub mod handlers;
pub mod url_utils;

pub use ::config::ConfigError;
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
