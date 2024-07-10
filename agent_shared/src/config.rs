use config::ConfigError;
use serde::Deserialize;
use std::{collections::HashMap, sync::Mutex};
use tracing::info;

#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationConfiguration {
    pub log_format: String,
    pub url: String,
    pub base_path: Option<String>,
    pub did_methods: HashMap<String, DidMethodOptions>,
    pub external_server_response_timeout_ms: Option<u64>,
}

// pub enum DidMethod {
//     #[serde(rename = "did:jwk")]
//     Jwk,
//     Key,
//     Web,
//     IotaRms,
// }

#[derive(Debug, Deserialize, Default, Clone)]
pub struct DidMethodOptions {
    pub enabled: bool,
    pub preferred: Option<bool>,
}

static CONFIG: Mutex<Option<ApplicationConfiguration>> = Mutex::new(None);

impl ApplicationConfiguration {
    pub fn new() -> Result<Self, ConfigError> {
        info!("Loading application configuration ...");
        let config = config::Config::builder()
            .add_source(config::File::with_name("agent_application/example-config.yaml"))
            .add_source(config::Environment::with_prefix("AGENT").separator("__"))
            .build()?;
        config.try_deserialize()
    }
}

/// Loads the configuration or returns it, if it has already been loaded.
pub fn config_2() -> ApplicationConfiguration {
    CONFIG
        .lock()
        .unwrap()
        .get_or_insert_with(|| ApplicationConfiguration::new().unwrap())
        .clone()
}

/// Read environment variables
#[allow(unused)]
pub fn config(package_name: &str) -> config::Config {
    #[cfg(feature = "test_utils")]
    let config = test_config();

    #[cfg(not(feature = "test_utils"))]
    let config = {
        dotenvy::dotenv().ok();

        config::Config::builder()
            // .add_source(config::Environment::with_prefix(package_name))
            // TODO: read config.{run_mode}.yml from env "RUN_MODE"
            .add_source(config::File::with_name("agent_application/example-config.yaml"))
            .add_source(config::Environment::with_prefix("AGENT"))
            .build()
            .unwrap()
    };

    // TODO: this should ideally only printed once on startup or when the config changed
    // info!("{:#?}", config.clone().try_deserialize::<serde_yaml::Value>().unwrap());

    config
}

/// Read environment variables for tests that can be used across packages
#[cfg(feature = "test_utils")]
fn test_config() -> config::Config {
    use crate::{issuance::TEST_ISSUER_CONFIG, metadata::TEST_METADATA};
    use std::env;

    dotenvy::from_filename("agent_shared/tests/.env.test").ok();

    env::remove_var("AGENT_APPLICATION_BASE_PATH");

    let mut config_builder = config::Config::builder().add_source(config::Environment::with_prefix("TEST"));

    // If some test metadata configuration is set then add it to the global configuration.
    let metadata = TEST_METADATA.lock().unwrap();
    if let Some(metadata) = metadata.as_ref() {
        let metadata_string = serde_yaml::to_string(metadata).unwrap();
        config_builder = config_builder.add_source(config::File::from_str(&metadata_string, config::FileFormat::Yaml));
    }

    config_builder = config_builder.add_source(config::File::from_str(
        &serde_yaml::to_string(&*TEST_ISSUER_CONFIG).unwrap(),
        config::FileFormat::Yaml,
    ));

    config_builder.build().unwrap()
}
