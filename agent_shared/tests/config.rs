use agent_shared::config;

#[cfg(feature = "test")]
#[test]
fn test_config() {
    assert_eq!(config!("variable", String).unwrap(), "env_value");
    assert_eq!(config!("global_variable", String).unwrap(), "global_env_value");
    // Reading from an environment variable that belongs to another package should fail.
    assert!(config!("other_variable", String).is_err());
}
