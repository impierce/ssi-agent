use std::{collections::HashMap, fs};

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

    // Load the appropriate .env file
    if cfg!(feature = "test_utils") {
        let env_test = load_env_file_variables("../.env.test");
        // Use the map as the environment source
        builder = builder.add_source(
            config::Environment::with_prefix("UNICORE")
                .separator("__")
                .source(Some(env_test)),
        );
    }

    builder = builder.add_source(config::Environment::with_prefix("UNICORE").separator("__"));

    let config = builder.build()?;

    Ok(config)
}

// We avoid using the `dotenvy` crate here because it sets environment variables globally,
// which can cause interference between tests (even when run serially) if one test sets a variable
// that another test expects to be unset. Instead, we manually load the .env file into a HashMap
// and provide it directly to the config builder. This approach ensures test isolation by not
// modifying the global process environment.
#[cfg(feature = "test_utils")]
fn load_env_file_variables(path: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let content = fs::read_to_string(path).expect("Failed to read .env file");
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(
                key.trim().to_string(),
                value.trim_start_matches('"').trim_end_matches('"').to_string(),
            );
        }
    }
    map
}
