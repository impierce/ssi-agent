use tracing::info;

/// Read environment variables
#[allow(unused)]
pub fn config(package_name: &str) -> config::Config {
    #[cfg(feature = "test")]
    let config = test_config();

    #[cfg(not(feature = "test"))]
    let config = {
        dotenvy::dotenv().ok();

        config::Config::builder()
            .add_source(config::Environment::with_prefix(package_name))
            .add_source(config::Environment::with_prefix("AGENT_CONFIG"))
            .add_source(config::File::with_name("agent_application/config.yml"))
            .add_source(config::File::with_name("agent_issuance/issuance-config.yml"))
            .build()
            .unwrap()
    };

    info!("{:?}", config);

    config
}

/// Read environment variables for tests that can be used across packages
#[cfg(feature = "test")]
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

    // If some test issuer configuration is set then add it to the global configuration.
    let issuer_config = TEST_ISSUER_CONFIG.lock().unwrap();

    if let Some(issuer_config) = issuer_config.as_ref() {
        let issuer_config_string = serde_yaml::to_string(issuer_config).unwrap();
        config_builder =
            config_builder.add_source(config::File::from_str(&issuer_config_string, config::FileFormat::Yaml));
    }

    config_builder.build().unwrap()
}
