mod events;
pub mod user;

use agent_storage::EventReadModel;
use anyhow::Result;
use identity_core::{common::Url, convert::FromJson};
use identity_credential::credential::{Credential, CredentialBuilder, Subject};
use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::JwtVcJson;
use oid4vci::credential_format_profiles::w3c_verifiable_credentials::ldp_vc::{self, LdpVc};
use oid4vci::credential_format_profiles::{CredentialFormatCollection, CredentialFormats, Parameters};
// use oid4vci::credential_format_profiles::CredentialFormats::JwtVcJson;
use oid4vci::credential_offer::{CredentialOffer, CredentialOfferQuery, CredentialsObject, Grants, PreAuthorizedCode};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;

use crate::events::CredentialEvent;
use crate::user::User;

// #[derive(Debug, PartialEq)]
// pub struct Credential {}

pub async fn init() -> Result<()> {
    agent_storage::get_all().await?;
    Ok(())
}

type Id = String;

/// Command: Create a new unsigned `Credential` for the given subject
pub async fn create_credential(
    user: Option<User>,
    credential_subject: Value,
) -> Result<(Id, identity_credential::credential::Credential)> {
    // A user is currently optional
    let user_id = match user {
        Some(User::Basic(basic)) => Some(basic.username),
        Some(User::Jwt(jwt)) => todo!(),
        None => None,
    };

    let uuid = uuid::Uuid::new_v4();

    let credential: identity_credential::credential::Credential = CredentialBuilder::default()
        .id(Url::parse(format!("http://localhost:3033/credentials/{}", uuid))?)
        .issuer(Url::parse("https://example.com")?)
        .subject(Subject::from_json_value(credential_subject.clone())?)
        .build()?;

    let event_0 = CredentialEvent::CredentialCreated {
        user_id: user_id.clone(),
        timestamp: now(),
        payload: serde_json::to_value(&credential)?,
    };

    let id = agent_storage::append_event(serde_json::to_value(event_0)?, Some(uuid.into())).await?;

    dbg!(&id);

    Ok((id, credential))
}

pub async fn sign_credential(id: Id) -> Result<identity_credential::credential::Credential> {
    let events = agent_storage::get_all().await?;
    let event: Vec<&agent_storage::EventReadModel> = events.iter().filter(|e| e.id.id.to_raw() == id).collect();

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

    let credential_signed =
        <agent_key_manager::kms::stronghold::StrongholdKeyManager as agent_key_manager::KeyManager>::sign(
            credential.clone(),
        )
        .unwrap();

    dbg!(&credential_signed);

    // TODO: let identity.rs sign it, provide existing stronghold

    let credential_signed_event = CredentialEvent::CredentialSigned {
        user_id: None,
        timestamp: now(),
        payload: json!({
            "credential_id": id,
            "key_id": "1337",
            "data": credential_signed,
        }),
    };
    agent_storage::append_event(serde_json::to_value(credential_signed_event)?, None).await?;

    Ok(credential_signed)
}

pub async fn create_credential_offer(credential_ids: Vec<uuid::Uuid>) -> Result<String> {
    dbg!(&credential_ids);
    let offer: CredentialOffer<CredentialFormats> = CredentialOffer {
        credential_issuer: "http://Daniels-Macbook-Pro.local:3033".parse().unwrap(),
        credentials: vec![CredentialsObject::ByValue(CredentialFormats::LdpVc(Parameters {
            format: LdpVc,
            parameters: (
                ldp_vc::CredentialDefinition {
                    context: vec![
                        "https://www.w3.org/2018/credentials/v1".to_string(),
                        "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json".to_string(),
                    ],
                    type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                    credential_subject: None,
                },
                None,
            )
                .into(),
        }))],
        grants: Some(Grants {
            authorization_code: None,
            pre_authorized_code: Some(PreAuthorizedCode {
                pre_authorized_code: "1337".to_string(),
                user_pin_required: false,
                interval: None,
            }),
        }),
    };

    let query: CredentialOfferQuery<CredentialFormats> = CredentialOfferQuery::CredentialOffer(offer.clone());

    dbg!(&offer);
    // dbg!(&query.to_string());

    Ok(query.to_string())
}

pub async fn get_credential(id: String) -> Option<Credential> {
    let events = agent_storage::get_all().await.unwrap();
    let result: Vec<&EventReadModel> = events.iter().filter(|e| e.id.id.to_raw() == id).collect();

    result.first().and_then(|e| {
        let payload = e.data.as_object().unwrap().get("payload").unwrap().to_owned();
        Some(Credential::from_json_value(payload).unwrap())
    })

    // let s = result
    //     .first()
    //     .unwrap_or(None)
    //     .data
    //     .as_object()
    //     .unwrap()
    //     .get("payload")
    //     .unwrap()
    //     .to_owned();
    // Some(Credential::from_json_value(s).unwrap())
}

pub async fn get_all_credential_events() -> Result<Value> {
    let events = agent_storage::get_all().await?;
    Ok(events.iter().map(|e| serde_json::to_value(e).unwrap()).collect())
}

/// Returns the current system time
fn now() -> String {
    // let format = time::format_description::well_known::Rfc3339;
    // let format = FormatDescription::parse(well_known::Rfc3339).unwrap();
    let current_time = time::OffsetDateTime::now_utc();
    current_time.format(&Rfc3339).unwrap()
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
