use tracing::{info, warn};

/// Loads provisioned configuration from a yaml file and environment variables.
pub fn load_provisioned_config() -> Result<config::Config, config::ConfigError> {
    let mut builder = config::Config::builder();

    let config_file_path_str = std::env::var("UNICORE__CONFIG_FILE").unwrap_or_else(|_| {
        if cfg!(feature = "test_utils") {
            "../agent_shared/tests/test.config.yaml".to_string()
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

        Ok(config)
    }
    #[cfg(not(feature = "test_utils"))]
    {
        // Load the appropriate .env file
        dotenvy::dotenv().ok();

        let config = builder
            .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
            .build()?;

        Ok(config)
    }
}
