mod events;
pub mod user;

use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use identity_core::{common::Url, convert::FromJson};
use identity_credential::credential::{CredentialBuilder, Subject};
use iota_stronghold::procedures::{Ed25519Sign, GenerateKey, KeyType, StrongholdProcedure};
use iota_stronghold::Location;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::events::CredentialEvent;
use crate::user::User;

#[derive(Debug, PartialEq)]
pub struct Credential {}

pub async fn init() -> Result<()> {
    agent_storage::get_all().await?;
    Ok(())
}

type Id = String;

/// Command: Create a new unsigned `Credential` for the given subject
pub async fn create_credential(user: Option<User>, credential_subject: Value) -> Result<Id> {
    // A user is currently optional
    let user_id = match user {
        Some(User::Basic(basic)) => Some(basic.username),
        Some(User::Jwt(jwt)) => todo!(),
        None => None,
    };

    let credential: identity_credential::credential::Credential = CredentialBuilder::default()
        .issuer(Url::parse("https://example.com")?)
        .subject(Subject::from_json_value(credential_subject.clone())?)
        .build()?;

    let event_0 = CredentialEvent::CredentialCreated {
        user_id: user_id.clone(),
        timestamp: time::OffsetDateTime::now_utc().to_string(),
        payload: serde_json::to_value(credential)?,
    };

    let id = agent_storage::append_event(serde_json::to_value(event_0)?).await?;

    dbg!(&id);

    Ok(id)
}

pub async fn sign_credential(id: Id) -> Result<()> {
    let stronghold = iota_stronghold::Stronghold::default();
    let client = stronghold.create_client("client_path_0")?;
    client
        .execute_procedure(StrongholdProcedure::GenerateKey(GenerateKey {
            ty: KeyType::Ed25519,
            output: Location::counter("client_path_0".as_bytes(), 0u8),
        }))
        .expect("failed to generate new private key");

    let events = agent_storage::get_all().await?;
    let event: Vec<&agent_storage::EventReadModel> = events.iter().filter(|e| e.id.id.to_string() == id).collect();
    // dbg!(&event);

    let credential: identity_credential::credential::Credential =
        identity_credential::credential::Credential::from_json_value(
            event
                .first()
                .unwrap()
                .data
                .as_object()
                .unwrap()
                .get("payload")
                .unwrap()
                .to_owned(),
        )
        .unwrap();

    dbg!(&credential);

    // TODO: let identity.rs sign it, provide existing stronghold

    let procedure_result = client.execute_procedure(StrongholdProcedure::Ed25519Sign(Ed25519Sign {
        private_key: Location::counter("client_path_0", 0u8),
        msg: credential.to_string().as_bytes().to_vec(),
    }))?;

    let output: Vec<u8> = procedure_result.into();
    println!("sig (base64): \"{}\"", general_purpose::STANDARD.encode(&output));

    // let event_1 = CredentialEvent::CredentialSigned {
    //     user_id: user_id.clone(),
    //     timestamp: time::OffsetDateTime::now_utc().to_string(),
    //     key_id: "1337".to_string(), // TODO
    // };

    // agent_storage::append_event(serde_json::to_value(event_1)?).await?;

    Ok(())
}

pub async fn get_all_credential_events() -> Result<Value> {
    let events = agent_storage::get_all().await?;
    Ok(events.iter().map(|e| serde_json::to_value(e).unwrap()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successfully_creates() {
        let result = create_credential(
            None,
            json!({
                "first_name":"Clark",
                "last_name": "Kent",
            }),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn successfully_signs_with_the_default_key() {
        let result = sign_credential();
        assert!(result.is_ok());
    }
}
