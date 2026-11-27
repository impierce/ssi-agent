use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

static LEGACY_DOMAIN_LINKAGE_SETTING: AtomicBool = AtomicBool::new(false);
static LEGACY_EVENT_PUBLISHERS_SETTING: AtomicBool = AtomicBool::new(false);

pub fn warn_deprecated_settings() {
    if LEGACY_DOMAIN_LINKAGE_SETTING.load(Ordering::Relaxed) {
        warn!("domain_linkage_enabled is deprecated and ignored; manage domain linkage through the API");
    }
    if LEGACY_EVENT_PUBLISHERS_SETTING.load(Ordering::Relaxed) {
        warn!("event_publishers is deprecated and ignored; manage event publishers through the API");
    }
}

fn has_legacy_event_publishers_setting(config: &config::Config) -> bool {
    config.get::<config::Value>("event_publishers").is_ok()
        || std::env::vars_os().any(|(key, _)| {
            key.to_str().is_some_and(|key| {
                let upper = key.to_ascii_uppercase();
                upper == "UNICORE__EVENT_PUBLISHERS" || upper.starts_with("UNICORE__EVENT_PUBLISHERS__")
            })
        })
}

fn record_deprecated_settings(config: &config::Config) {
    if config.get::<config::Value>("domain_linkage_enabled").is_ok() {
        LEGACY_DOMAIN_LINKAGE_SETTING.store(true, Ordering::Relaxed);
    }
    if has_legacy_event_publishers_setting(config) {
        LEGACY_EVENT_PUBLISHERS_SETTING.store(true, Ordering::Relaxed);
    }
}

/// Loads provisioned configuration from a yaml file and environment variables.
pub fn load_provisioned_config() -> Result<config::Config, config::ConfigError> {
    let mut builder = config::Config::builder();

    let config_file_path_str = std::env::var("UNICORE__CONFIG_FILE").unwrap_or_else(|_| {
        if cfg!(feature = "test_utils") {
            // Uses compile-time CARGO_MANIFEST_DIR to ensure tests locate test.config.yaml
            // regardless of the working directory across workspaces.
            // See `docs/adr/0003-hermetic-test-architecture-and-config-decoupling.md` for context.
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/test.config.yaml").to_string()
        } else {
            "./config.yaml".to_string()
        }
    });

    let config_file_path = std::path::Path::new(&config_file_path_str);

    if config_file_path.exists() {
        builder = builder.add_source(config::File::with_name(&config_file_path_str));
        println!("Loaded config file: `{}`", config_file_path.display());
        info!("Loaded config file: `{}`", config_file_path.display());
    } else {
        println!("Config file not found: `{}`", config_file_path.display());
        warn!("Config file not found: `{}`", config_file_path.display());
    }

    #[cfg(feature = "test_utils")]
    {
        let config = temp_env::with_vars(
            [
                (
                    "UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD",
                    Some("VNvRtH4tKyWwvJDpL6Vuc2aoLiKAecGQ"),
                ),
                ("UNICORE__EVENT_STORE__CONNECTION_STRING", Some("postgresql://:test:")),
            ],
            || {
                builder
                    .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                    .build()
            },
        )?;

        record_deprecated_settings(&config);
        Ok(config)
    }
    #[cfg(not(feature = "test_utils"))]
    {
        // Load the appropriate .env file
        dotenvy::dotenv().ok();

        let config = builder
            .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
            .build()?;

        record_deprecated_settings(&config);
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn detects_legacy_event_publishers_from_yaml_and_env() {
        let empty = config::Config::builder().build().unwrap();
        assert!(!has_legacy_event_publishers_setting(&empty));

        let yaml_with_legacy = config::Config::builder()
            .add_source(config::File::from_str(
                "event_publishers:\n  http:\n    target_url: https://secret-token@example.test\n",
                config::FileFormat::Yaml,
            ))
            .build()
            .unwrap();
        assert!(has_legacy_event_publishers_setting(&yaml_with_legacy));

        temp_env::with_var(
            "UNICORE__EVENT_PUBLISHERS__HTTP__TARGET_URL",
            Some("https://secret-token@example.test"),
            || {
                assert!(has_legacy_event_publishers_setting(&empty));
            },
        );

        temp_env::with_var("UNICORE__EVENT_PUBLISHERS", Some("secret-token"), || {
            assert!(has_legacy_event_publishers_setting(&empty));
        });

        temp_env::with_var("unicore__event_publishers__nats__enabled", Some("true"), || {
            assert!(has_legacy_event_publishers_setting(&empty));
        });

        // Unrelated keys sharing the prefix without the `__` hierarchy separator must not match.
        temp_env::with_var("UNICORE__EVENT_PUBLISHERS_MAX_COUNT", Some("10"), || {
            assert!(!has_legacy_event_publishers_setting(&empty));
        });

        // End-to-end provisioned config load succeeds with legacy env var and records the warning flag.
        LEGACY_EVENT_PUBLISHERS_SETTING.store(false, Ordering::Relaxed);
        temp_env::with_var(
            "UNICORE__EVENT_PUBLISHERS__HTTP__0__TARGET_URL",
            Some("https://secret-token@example.test"),
            || {
                let loaded = load_provisioned_config().unwrap();
                assert!(LEGACY_EVENT_PUBLISHERS_SETTING.load(Ordering::Relaxed));
                let _ = crate::config::ApplicationConfiguration::load(
                    loaded,
                    crate::config::ApplicationProfile::Development,
                )
                .unwrap();
            },
        );
        LEGACY_EVENT_PUBLISHERS_SETTING.store(false, Ordering::Relaxed);
    }
}
