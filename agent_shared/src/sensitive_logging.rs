//! Log fields for values that may contain secrets or personal data: tokens, authorization and pre-authorized codes,
//! credential offers, credentials and email addresses.
//!
//! The fields are only recorded when `dev.log_sensitive_values` is enabled, which the `production` profile ignores, so
//! they never reach the logs of a production deployment, whatever the log level. Otherwise the field is left out of
//! the event.
//!
//! ```ignore
//! debug!(path = location.path(), location = sensitive(&location), "Redirecting after consent");
//! ```

use crate::config::log_sensitive_values;
use std::fmt;
use tracing::field::{debug, display, DebugValue, DisplayValue};

/// Records `value` with its `Display` implementation if sensitive values are logged.
pub fn sensitive<T: fmt::Display>(value: T) -> Option<DisplayValue<T>> {
    log_sensitive_values().then(|| display(value))
}

/// Records `value` with its `Debug` implementation if sensitive values are logged.
pub fn sensitive_debug<T: fmt::Debug>(value: T) -> Option<DebugValue<T>> {
    log_sensitive_values().then(|| debug(value))
}

/// Enables `dev.log_sensitive_values` until dropped. The setting is global, so tests that use this guard, and tests
/// that assert that sensitive values are not logged, must not run concurrently (`#[serial_test::serial]`).
#[cfg(any(test, feature = "test_utils"))]
pub struct SensitiveValuesLogged;

#[cfg(any(test, feature = "test_utils"))]
impl SensitiveValuesLogged {
    pub fn enable() -> Self {
        crate::config::config_mut().dev.log_sensitive_values = true;
        Self
    }
}

#[cfg(any(test, feature = "test_utils"))]
impl Drop for SensitiveValuesLogged {
    fn drop(&mut self) {
        crate::config::config_mut().dev.log_sensitive_values = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn sensitive_values_are_only_recorded_when_enabled() {
        assert!(sensitive("secret").is_none());
        assert!(sensitive_debug("secret").is_none());

        let enabled = SensitiveValuesLogged::enable();
        assert_eq!(
            sensitive("secret").map(|value| format!("{value:?}")).as_deref(),
            Some("secret")
        );
        assert_eq!(
            sensitive_debug("secret").map(|value| format!("{value:?}")).as_deref(),
            Some("\"secret\"")
        );

        drop(enabled);
        assert!(sensitive("secret").is_none());
    }
}
