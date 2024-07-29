use agent_shared::config::{config, SecretManagerConfig};
use did_manager::SecretManager;

pub mod subject;

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let SecretManagerConfig {
        stronghold_path: snapshot_path,
        stronghold_password: password,
        issuer_eddsa_key_id,
        issuer_es256_key_id,
        issuer_did,
        issuer_fragment,
    } = config().secret_manager.clone();

    match (
        snapshot_path,
        password,
        issuer_eddsa_key_id,
        issuer_es256_key_id,
        issuer_did,
        issuer_fragment,
    ) {
        (snapshot_path, password, issuer_eddsa_key_id, issuer_es256_key_id, issuer_did, issuer_fragment)
            if issuer_eddsa_key_id.is_some() || issuer_es256_key_id.is_some() =>
        {
            SecretManager::load(
                snapshot_path,
                password,
                issuer_eddsa_key_id,
                issuer_es256_key_id,
                None,
                issuer_did,
                issuer_fragment,
            )
            .await
            .unwrap()
        }
        (snapshot_path, password, None, None, _, _) => SecretManager::generate(snapshot_path, password).await.unwrap(),
        _ => panic!(),
    }
}
