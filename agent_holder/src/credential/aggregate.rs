use agent_shared::config::{config, get_preferred_did_method, get_preferred_signing_algorithm};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use identity_core::convert::FromJson;
use identity_credential::credential::{
    Credential as W3CVerifiableCredential, CredentialBuilder as W3CVerifiableCredentialBuilder, Issuer,
};
use jsonwebtoken::Header;
use oid4vc_core::jwt;
use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::{
    CredentialDefinition, JwtVcJson, JwtVcJsonParameters,
};
use oid4vci::credential_format_profiles::{CredentialFormats, Parameters};
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use oid4vci::credential_response::CredentialResponseType;
use oid4vci::VerifiableCredentialJwt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;
use types_ob_v3::prelude::{
    AchievementCredential, AchievementCredentialBuilder, AchievementCredentialType, AchievementSubject, Profile,
    ProfileBuilder,
};

use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self};
use crate::credential::event::CredentialEvent;
use crate::services::HolderServices;

use super::entity::Data;

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Credential {
    pub credential_id: Option<String>,
    pub offer_id: Option<String>,
    pub credential: Option<serde_json::Value>,
}

#[async_trait]
impl Aggregate for Credential {
    type Command = CredentialCommand;
    type Event = CredentialEvent;
    type Error = CredentialError;
    type Services = Arc<HolderServices>;

    fn aggregate_type() -> String {
        "credential".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use CredentialCommand::*;
        use CredentialError::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            AddCredential {
                credential_id,
                offer_id,
                credential,
            } => Ok(vec![CredentialAdded {
                credential_id,
                offer_id,
                credential,
            }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            CredentialAdded {
                credential_id,
                offer_id,
                credential,
            } => {
                self.credential_id = Some(credential_id);
                self.offer_id = Some(offer_id);
                self.credential = Some(credential);
            }
        }
    }
}

// #[cfg(test)]
// pub mod credential_tests {
//     use std::collections::HashMap;

//     use super::*;

//     use jsonwebtoken::Algorithm;
//     use lazy_static::lazy_static;
//     use oid4vci::proof::KeyProofMetadata;
//     use oid4vci::ProofType;
//     use rstest::rstest;
//     use serde_json::json;

//     use cqrs_es::test::TestFramework;

//     use crate::credential::aggregate::Credential;
//     use crate::credential::event::CredentialEvent;
//     use crate::offer::aggregate::tests::SUBJECT_KEY_DID;
//     use crate::services::test_utils::test_issuance_services;

//     type CredentialTestFramework = TestFramework<Credential>;

//     #[rstest]
//     #[case::openbadges(
//         OPENBADGE_CREDENTIAL_SUBJECT.clone(),
//         OPENBADGE_CREDENTIAL_CONFIGURATION.clone(),
//         UNSIGNED_OPENBADGE_CREDENTIAL.clone()
//     )]
//     #[case::w3c_vc(
//         W3C_VC_CREDENTIAL_SUBJECT.clone(),
//         W3C_VC_CREDENTIAL_CONFIGURATION.clone(),
//         UNSIGNED_W3C_VC_CREDENTIAL.clone()
//     )]
//     #[serial_test::serial]
//     fn test_create_unsigned_credential(
//         #[case] credential_subject: serde_json::Value,
//         #[case] credential_configuration: CredentialConfigurationsSupportedObject,
//         #[case] unsigned_credential: serde_json::Value,
//     ) {
//         CredentialTestFramework::with(test_issuance_services())
//             .given_no_previous_events()
//             .when(CredentialCommand::CreateUnsignedCredential {
//                 data: Data {
//                     raw: credential_subject,
//                 },
//                 credential_configuration: credential_configuration.clone(),
//             })
//             .then_expect_events(vec![CredentialEvent::UnsignedCredentialCreated {
//                 data: Data {
//                     raw: unsigned_credential,
//                 },
//                 credential_configuration,
//             }])
//     }

//     #[rstest]
//     #[case::openbadges(
//         UNSIGNED_OPENBADGE_CREDENTIAL.clone(),
//         OPENBADGE_CREDENTIAL_CONFIGURATION.clone(),
//         OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string(),
//     )]
//     #[case::w3c_vc(
//         UNSIGNED_W3C_VC_CREDENTIAL.clone(),
//         W3C_VC_CREDENTIAL_CONFIGURATION.clone(),
//         W3C_VC_VERIFIABLE_CREDENTIAL_JWT.to_string(),
//     )]
//     #[serial_test::serial]
//     async fn test_sign_credential(
//         #[case] unsigned_credential: serde_json::Value,
//         #[case] credential_configuration: CredentialConfigurationsSupportedObject,
//         #[case] verifiable_credential_jwt: String,
//     ) {
//         CredentialTestFramework::with(test_issuance_services())
//             .given(vec![CredentialEvent::UnsignedCredentialCreated {
//                 data: Data {
//                     raw: unsigned_credential,
//                 },
//                 credential_configuration,
//             }])
//             .when(CredentialCommand::SignCredential {
//                 subject_id: SUBJECT_KEY_DID.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
//                 overwrite: false,
//             })
//             .then_expect_events(vec![CredentialEvent::CredentialSigned {
//                 signed_credential: json!(verifiable_credential_jwt),
//             }])
//     }

//     lazy_static! {
//         static ref OPENBADGE_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
//             CredentialConfigurationsSupportedObject {
//                 credential_format: CredentialFormats::JwtVcJson(Parameters {
//                     parameters: (
//                         CredentialDefinition {
//                             type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
//                             credential_subject: Default::default(),
//                         },
//                         None,
//                     )
//                         .into(),
//                 }),
//                 cryptographic_binding_methods_supported: vec![
//                     "did:key".to_string(),
//                     "did:key".to_string(),
//                     "did:iota:rms".to_string(),
//                     "did:jwk".to_string(),
//                 ],
//                 credential_signing_alg_values_supported: vec!["EdDSA".to_string()],
//                 proof_types_supported: HashMap::from_iter(vec![(
//                     ProofType::Jwt,
//                     KeyProofMetadata {
//                         proof_signing_alg_values_supported: vec![Algorithm::EdDSA],
//                     },
//                 )]),
//                 display: vec![json!({
//                     "name": "Teamwork Badge",
//                     "logo": {
//                         "url": "https://example.com/logo.png"
//                     }
//                 })],
//                 ..Default::default()
//             };
//         static ref W3C_VC_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
//             CredentialConfigurationsSupportedObject {
//                 credential_format: CredentialFormats::JwtVcJson(Parameters {
//                     parameters: (
//                         CredentialDefinition {
//                             type_: vec!["VerifiableCredential".to_string()],
//                             credential_subject: Default::default(),
//                         },
//                         None,
//                     )
//                         .into(),
//                 }),
//                 cryptographic_binding_methods_supported: vec![
//                     "did:key".to_string(),
//                     "did:key".to_string(),
//                     "did:iota:rms".to_string(),
//                     "did:jwk".to_string(),
//                 ],
//                 credential_signing_alg_values_supported: vec!["EdDSA".to_string()],
//                 proof_types_supported: HashMap::from_iter(vec![(
//                     ProofType::Jwt,
//                     KeyProofMetadata {
//                         proof_signing_alg_values_supported: vec![Algorithm::EdDSA],
//                     },
//                 )]),
//                 display: vec![json!({
//                     "name": "Master Degree",
//                     "logo": {
//                         "url": "https://example.com/logo.png"
//                     }
//                 })],
//                 ..Default::default()
//             };
//         static ref OPENBADGE_CREDENTIAL_SUBJECT: serde_json::Value = json!(
//             {
//                 "credentialSubject": {
//                     "type": [ "AchievementSubject" ],
//                     "achievement": {
//                               "id": "https://example.com/achievements/21st-century-skills/teamwork",
//                               "type": "Achievement",
//                               "criteria": {
//                                   "narrative": "Team members are nominated for this badge by their peers and recognized upon review by Example Corp management."
//                               },
//                               "description": "This badge recognizes the development of the capacity to collaborate within a group environment.",
//                               "name": "Teamwork"
//                           }
//                   }
//             }
//         );
//         static ref W3C_VC_CREDENTIAL_SUBJECT: serde_json::Value = json!(
//             {
//                 "credentialSubject": {
//                     "first_name": "Ferris",
//                     "last_name": "Rustacean",
//                     "degree": {
//                         "type": "MasterDegree",
//                         "name": "Master of Oceanography"
//                     }
//                 }
//             }
//         );
//         static ref UNSIGNED_OPENBADGE_CREDENTIAL: serde_json::Value = json!({
//           "@context": [
//             "https://www.w3.org/2018/credentials/v1",
//             "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
//           ],
//           "id": "http://example.com/credentials/3527",
//           "type": ["VerifiableCredential", "OpenBadgeCredential"],
//           "issuer": {
//             "id": "https://my-domain.example.org",
//             "type": "Profile",
//             "name": "UniCore"
//           },
//           "issuanceDate": "2010-01-01T00:00:00Z",
//           "name": "Teamwork Badge",
//           "credentialSubject": OPENBADGE_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
//         });
//         static ref UNSIGNED_W3C_VC_CREDENTIAL: serde_json::Value = json!({
//           "@context": "https://www.w3.org/2018/credentials/v1",
//           "type": [ "VerifiableCredential" ],
//           "credentialSubject": W3C_VC_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
//           "issuer": {
//             "id": "https://my-domain.example.org/",
//             "name": "UniCore"
//           },
//           "issuanceDate": "2010-01-01T00:00:00Z"
//         });
//     }

//     pub const OPENBADGE_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3AwL2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZmlhYmxlQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5hbWUiOiJUZWFtd29yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJ0eXBlIjpbIkFjaGlldmVtZW50U3ViamVjdCJdLCJhY2hpZXZlbWVudCI6eyJpZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vYWNoaWV2ZW1lbnRzLzIxc3QtY2VudHVyeS1za2lsbHMvdGVhbXdvcmsiLCJ0eXBlIjoiQWNoaWV2ZW1lbnQiLCJjcml0ZXJpYSI6eyJuYXJyYXRpdmUiOiJUZWFtIG1lbWJlcnMgYXJlIG5vbWluYXRlZCBmb3IgdGhpcyBiYWRnZSBieSB0aGVpciBwZWVycyBhbmQgcmVjb2duaXplZCB1cG9uIHJldmlldyBieSBFeGFtcGxlIENvcnAgbWFuYWdlbWVudC4ifSwiZGVzY3JpcHRpb24iOiJUaGlzIGJhZGdlIHJlY29nbml6ZXMgdGhlIGRldmVsb3BtZW50IG9mIHRoZSBjYXBhY2l0eSB0byBjb2xsYWJvcmF0ZSB3aXRoaW4gYSBncm91cCBlbnZpcm9ubWVudC4iLCJuYW1lIjoiVGVhbXdvcmsifX19fQ.SkC7IvpBGB9e98eobnE9qcLjs-yoZup3cieBla3DRTlcRezXEDPv4YRoUgffho9LJ0rkmfFPsPwb-owXMWyPAA";

//     pub const W3C_VC_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjoiaHR0cHM6Ly93d3cudzMub3JnLzIwMTgvY3JlZGVudGlhbHMvdjEiLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJmaXJzdF9uYW1lIjoiRmVycmlzIiwibGFzdF9uYW1lIjoiUnVzdGFjZWFuIiwiZGVncmVlIjp7InR5cGUiOiJNYXN0ZXJEZWdyZWUiLCJuYW1lIjoiTWFzdGVyIG9mIE9jZWFub2dyYXBoeSJ9fSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiJ9fQ.MUDBbPJfXe0G9sjVTF3RuR6ukRM0d4N57iMGNFcIKMFPIEdig12v-YFB0qfnSghGcQo8hUw3jzxZXTSJATEgBg";
// }
