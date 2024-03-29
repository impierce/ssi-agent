pub mod application_state;
pub mod config;
pub mod generic_query;
pub mod handlers;
pub mod secret_manager;
pub mod url_utils;

pub use ::config::ConfigError;
use rand::Rng;
pub use url_utils::UrlAppendHelpers;

/// Macro to read configuration using the package name as prefix.
#[macro_export]
macro_rules! config {
    ($string:expr) => {
        agent_shared::config::config(std::env!("CARGO_PKG_NAME")).get_string($string)
    };
}

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
