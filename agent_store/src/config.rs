/// Read environment variables
pub fn config() -> config::Config {
    // Load global .env file
    dotenvy::dotenv().ok();

    // Build configuration
    config::Config::builder()
        .add_source(config::Environment::with_prefix("AGENT_STORE"))
        .build()
        .unwrap()
}
