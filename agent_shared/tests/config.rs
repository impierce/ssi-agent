use agent_shared::config;

#[test]
fn test_config() {
    dotenvy::from_filename("tests/.env.test").ok();

    assert_eq!(config!("variable").unwrap(), "env_value");
    assert_eq!(config!("global_variable").unwrap(), "global_env_value");
    // Reading from an environment variable that belongs to another package should fail.
    assert!(config!("other_variable").is_err());
}
