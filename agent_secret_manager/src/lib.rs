use agent_shared::config;
use producer::did_document::Method;
use producer::SecretManager;

pub async fn init() -> Result<(), std::io::Error> {
    let snapshot_path = config!("stronghold_path").unwrap();
    let password = config!("stronghold_password").unwrap();

    let secret_manager = SecretManager::load(snapshot_path, password).unwrap();

    let signature = secret_manager.sign("foobar".as_bytes()).await.unwrap();
    println!("{:x?}", signature);

    let document = secret_manager.produce_document_json(Method::Web).await.unwrap();
    println!("{}", serde_json::to_string_pretty(&document).unwrap());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_load() {
        env::set_var("AGENT_SECRET_MANAGER_STRONGHOLD_PATH", "tests/res/test.stronghold");
        env::set_var("AGENT_SECRET_MANAGER_STRONGHOLD_PASSWORD", "secure_password");
        assert!(init().await.is_ok());
    }
}
