mod events;
mod persistence;

use anyhow::Result;
use identity_core::{common::Url, convert::FromJson};
use identity_credential::credential::{CredentialBuilder, Subject};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::events::CredentialEvent;
use crate::persistence::InMemory;

#[derive(Debug, PartialEq)]
pub struct Credential {}

/// Command: Create a new unsigned credential for the given subject
pub fn create_credential(credential_subject: Value) -> Result<Credential> {
    let event_store = InMemory::new();

    let user_id = Uuid::new_v4().to_string();

    let credential: identity_credential::credential::Credential = CredentialBuilder::default()
        .issuer(Url::parse("https://example.com")?)
        .subject(Subject::from_json_value(credential_subject.clone())?)
        .build()?;

    let event_0 = CredentialEvent::CredentialCreated {
        user_id: user_id.clone(),
        payload: serde_json::to_value(credential)?,
    };

    let event_1 = CredentialEvent::CredentialSigned {
        user_id: user_id.clone(),
        key_id: "1337".to_string(),
    };

    event_store.append(serde_json::to_value(event_0)?);
    event_store.append(serde_json::to_value(event_1)?);

    for (index, item) in event_store.get_all().iter().enumerate() {
        println!("{}: {:?}", index, item);
    }
    Ok(Credential {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successfully_creates() {
        let result = create_credential(json!({
            "first_name":"Clark",
            "last_name": "Kent",
        }));
        assert_eq!(result.unwrap(), Credential {});
    }
}
