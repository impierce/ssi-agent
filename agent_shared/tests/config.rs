use agent_shared::config;

#[test]
fn test_config() {
    dotenvy::from_filename("tests/.env.test").ok();

    assert_eq!(config!("env_variable").unwrap(), "env_value");
}
