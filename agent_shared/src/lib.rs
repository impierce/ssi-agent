pub mod config;
pub mod url_utils;

pub use ::config::ConfigError;
pub use url_utils::UrlAppendHelpers;

/// Macro to read configuration using the package name as prefix.
#[macro_export]
macro_rules! config {
    ($string:expr) => {
        agent_shared::config::config(std::env!("CARGO_PKG_NAME")).get_string($string)
    };
}
