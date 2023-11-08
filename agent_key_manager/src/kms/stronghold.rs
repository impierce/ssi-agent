use std::collections::BTreeMap;

use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use identity_credential::credential::{Credential, Proof};
use identity_storage::{JwkDocumentExt, JwkMemStore, Storage};
use identity_stronghold::StrongholdStorage;
use identity_verification::jws::JwsAlgorithm;
use identity_verification::MethodScope;
use iota_sdk::client::{
    secret::{stronghold::StrongholdSecretManager, SecretManager},
    Password,
};
use iota_stronghold::{
    procedures::{Ed25519Sign, GenerateKey, KeyType, StrongholdProcedure},
    Location,
};
use rand::distributions::DistString;

use crate::KeyManager;

pub struct StrongholdKeyManager {}

impl KeyManager for StrongholdKeyManager {
    fn create_verification_method() -> Result<String> {
        let password = Password::from("secure_password".to_owned());
        let path = random_stronghold_path();

        let stronghold = StrongholdSecretManager::builder()
            .password(password.clone())
            .build(path.clone())?;

        let stronghold_storage = StrongholdStorage::new(stronghold);
        let storage = Storage::new(stronghold_storage.clone(), stronghold_storage.clone());

        let mut core_document = identity_document::document::DocumentBuilder::default().build().unwrap();

        let fragment = core_document.generate_method(
            &storage,
            JwkMemStore::ED25519_KEY_TYPE,
            JwsAlgorithm::EdDSA,
            None,
            MethodScope::VerificationMethod,
        );

        Ok("1337".to_string())
    }

    fn sign(credential: Credential) -> Result<Credential> {
        let stronghold = iota_stronghold::Stronghold::default();
        let client = stronghold.create_client("client_path_0")?;
        client
            .execute_procedure(StrongholdProcedure::GenerateKey(GenerateKey {
                ty: KeyType::Ed25519,
                output: Location::counter("client_path_0".as_bytes(), 0u8),
            }))
            .expect("failed to generate new private key");
        dbg!("generated new private key");

        // Set proof (without signature)
        let mut credential = credential;
        credential.set_proof(Some(Proof::new(
            "Ed25519Signature2020".to_string(),
            BTreeMap::from([("created".to_string(), serde_json::to_value("_now")?)]),
        )));
        dbg!(&credential);

        let procedure_result = client.execute_procedure(StrongholdProcedure::Ed25519Sign(Ed25519Sign {
            private_key: Location::counter("client_path_0", 0u8),
            msg: credential.to_string().as_bytes().to_vec(),
        }))?;

        let output: Vec<u8> = procedure_result.into();

        // Set proof (including signature)
        credential.set_proof(Some(Proof::new(
            "Ed25519Signature2020".to_string(),
            BTreeMap::from([(
                "signature".to_string(),
                serde_json::to_value(general_purpose::STANDARD.encode(&output))?,
            )]),
        )));

        Ok(credential)
    }
}

fn random_stronghold_path() -> std::path::PathBuf {
    let mut file = std::env::temp_dir();
    file.push("test_strongholds");
    file.push(rand::distributions::Alphanumeric.sample_string(&mut rand::thread_rng(), 32));
    file.set_extension("stronghold");
    file.to_owned()
}
