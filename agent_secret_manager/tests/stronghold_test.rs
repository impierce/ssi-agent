use agent_secret_manager::SecretManager;
use identity_iota::storage::{JwkStorage, KeyId};

#[tokio::test]
async fn loads_existing_stronghold() {
    std::env::set_var("AGENT_SECRET_MANAGER_STRONGHOLD_PATH", "tests/res/test.stronghold");
    std::env::set_var("AGENT_SECRET_MANAGER_STRONGHOLD_PASSWORD", "secure_password");

    let secret_manager = SecretManager::load();

    assert!(secret_manager
        .stronghold_storage
        .exists(&KeyId::new("9O66nzWqYYy1LmmiOudOlh2SMIaUWoTS"))
        .await
        .unwrap());
}

#[tokio::test]
#[ignore]
async fn generates_new_stronghold() {
    iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();

    let _ = SecretManager::generate();

    // TODO: assert that the stronghold was created
}
