pub mod config;

/// Macro to read configuration using the package name as prefix.
#[macro_export]
macro_rules! config {
    ($string:expr) => {
        agent_shared::config::config(std::env!("CARGO_PKG_NAME")).get_string($string)
    };
}
