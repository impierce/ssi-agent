use tracing::{info, warn};

/// Loads provisioned configuration from a yaml file and environment variables.
pub fn load_provisioned_config() -> Result<config::Config, config::ConfigError> {
    let mut builder = config::Config::builder();

    // Load the appropriate .env file
    if cfg!(feature = "test_utils") {
        dotenvy::from_filename("../.env.test").ok();
    }

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

    builder = builder.add_source(config::Environment::with_prefix("UNICORE").separator("__"));

    let config = builder.build()?;

    Ok(config)
}
