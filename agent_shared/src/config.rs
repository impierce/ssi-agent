use tracing::info;

/// Read environment variables
#[cfg(feature = "test")] // Only allow unused code when testing
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
            .build()
            .unwrap()
    };

    info!("{:?}", config);

    config
}

/// Read environment variables for tests that can be used across packages
#[cfg(feature = "test")]
fn test_config() -> config::Config {
    dotenvy::from_filename("agent_shared/tests/.env.test").ok();

    config::Config::builder()
        .add_source(config::Environment::with_prefix("TEST"))
        .add_source(config::Environment::with_prefix("AGENT_CONFIG"))
        .build()
        .unwrap()
}
