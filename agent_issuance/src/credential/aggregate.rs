use super::entity::Data;
use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self};
use crate::credential::event::CredentialEvent;
use crate::services::IssuanceServices;
use agent_shared::config::{
    config, get_preferred_did_method, get_preferred_signing_algorithm, AlgorithmExt, BITS_PER_STATUS,
    STATUS_LIST_BYTES_AMOUNT,
};
use agent_shared::json_schema::CredentialType;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
use identity_core::convert::FromJson;
use identity_credential::sd_jwt_vc::{self, SdJwtVcBuilder, SdJwtVcClaims, StatusListRef, StatusMechanism};
use jsonwebtoken::Header;
use oauth_tsl::status_list::StatusType;
use oauth_tsl::tokens::status_list_token::StatusListTyp;
use oid4vc_core::{jwt, Sign as _, Subject as _};
use oid4vci::credential_format_profiles::ietf_sd_jwt_vc::dc_sd_jwt::{DcSdJwt, DcSdJwtParameters};
use oid4vci::credential_format_profiles::vc_jose_cose::vc_sd_jwt::{self, VcSdJwt, VcSdJwtParameters};
use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::{
    CredentialDefinition, JwtVcJson, JwtVcJsonParameters,
};
use oid4vci::credential_format_profiles::{CredentialFormats, Parameters};
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use oid4vci::notification_request::NotificationRequest;
use oid4vci::{Proof, VerifiableCredentialJwt};
use sd_jwt::{RequiredKeyBinding, SdJwtBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, info};
use url::Url;

// tmp
use identity_credential::credential::CredentialV2 as W3CVerifiableCredentialV2;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Pending,
    Issued,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum CredentialExpiry {
    Fixed(chrono::DateTime<chrono::Utc>),
    #[serde(with = "never_as_str")]
    Never,
}

mod never_as_str {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("never")
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "never" {
            Ok(())
        } else {
            Err(serde::de::Error::custom("expected 'never'"))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CredentialStatus {
    pub index: usize,
    pub status: StatusType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Credential {
    #[serde(rename = "id")]
    pub credential_id: String,
    pub notification_id: Option<String>,
    pub data: Option<Data>,
    pub credential_configuration: CredentialConfigurationsSupportedObject,
    pub signed: Option<serde_json::Value>,
    pub status: Status,
    pub holder_notifications: Vec<NotificationRequest>,
    pub credential_status: CredentialStatus,
    pub created_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl Aggregate for Credential {
    type Command = CredentialCommand;
    type Event = CredentialEvent;
    type Error = CredentialError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "credential".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use CredentialCommand::*;
        use CredentialError::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateUnsignedCredential {
                credential_id,
                data,
                credential_configuration,
                expires_at,
                credential_status_index,
            } => {
                #[cfg(feature = "test_utils")]
                let notification_id = test_utils::notification_id();
                #[cfg(not(feature = "test_utils"))]
                let notification_id = agent_shared::generate_random_string();

                #[cfg(feature = "test_utils")]
                let created_at: DateTime<Utc> = "2010-01-01T00:00:00Z"
                    .parse()
                    .map_err(|e| BuildCredentialError(format!("Failed to parse created_at: {}", e)))?;
                #[cfg(not(feature = "test_utils"))]
                let created_at: DateTime<Utc> = chrono::Utc::now();
                // .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

                let expires_at = match expires_at {
                    CredentialExpiry::Fixed(fixed) => Some(fixed),
                    CredentialExpiry::Never => None,
                };

                let credential_status = CredentialStatus {
                    index: credential_status_index,
                    status: StatusType::VALID,
                };

                let mut credential_data = data.raw.clone();

                // Add validFrom and validUntil as per VC DM 2.0
                //
                // This means we default to setting the `validFrom` to the creation date of the credential, which is a sensible default if no validFrom date has been entered.
                // However it is allowed to not enter any validity period and make an OBv3 credential valid eternally in to the past and/or future.
                // TODO: create a way to make a credential without a validFrom value.
                credential_data
                    .insert_if_none(
                        &["validFrom"],
                        json!(created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                    )
                    .ok_or(BuildCredentialError(
                        "Failed to enter the validFrom date into the credential".to_string(),
                    ))?;

                if let Some(expiration_date) = expires_at {
                    credential_data
                        .insert_at_path(&["expirationDate"], json!(expiration_date))
                        .ok_or(BuildCredentialError(
                            "Failed to enter the expirationDate into the credential".to_string(),
                        ))?;
                }

                // Add issuer
                let id = config().public_url.clone();
                let issuer_name = config().display.first().ok_or(InvalidCredentialDataError)?.name.clone();

                credential_data
                    .insert_at_path(
                        &["issuer"],
                        json!({
                            "id": id,
                            "name": issuer_name
                        }),
                    )
                    .ok_or(BuildCredentialError(
                        "Failed to enter the issuer into the credential".to_string(),
                    ))?;

                let credential_name = credential_configuration
                    .display
                    .first()
                    .map(|display| display.name.clone());

                // Add credential status
                let status_list_url = get_status_list_url(self.credential_status.index)?;

                credential_data.insert_if_none(
                    &["credentialStatus"],
                    json!({
                        "type": StatusListTyp::Jwt.to_string(),
                        "id": status_list_url.to_string(),
                        "uri": status_list_url.to_string(),
                        "idx": credential_status_index,
                    }),
                );

                match &credential_configuration.credential_format {
                    CredentialFormats::JwtVcJson(Parameters::<JwtVcJson> {
                        parameters:
                            JwtVcJsonParameters {
                                credential_definition: CredentialDefinition { type_, .. },
                                ..
                            },
                    }) => {
                        // TODO: we need to validate our own issuing against the referenced cred_config.
                        // Set the type to the original credential configuration type.
                        // TODO: And shouldn't users be able to type more specifically then only the cred config?
                        credential_data
                            .insert_at_path(&["type"], json!(type_))
                            .ok_or(BuildCredentialError(
                                "Failed to enter the type into the credential".to_string(),
                            ))?;

                        let mut credential_types = type_.clone();
                        // Loop through all the items in the `type` array in reverse until we find a match.
                        // This looping assumes the most specific type to match on is the latest one in the array.
                        // This is an implicit consequence of the typing rules in digital credential formats.
                        // For example, for OBv3 as well as ELM the first type is `VerifiableCredential` and the second type is its own type (e.g. `OpenBadgeCredential`/`EuropeanDigitalCredential`).
                        while let Some(credential_type) = credential_types.pop() {
                            match credential_type.as_str() {
                                // This supports VC DM 2.0 only.
                                "VerifiableCredential" => {
                                    credential_data
                                        .insert_if_none(&["@context"], json!(["https://www.w3.org/ns/credentials/v2"]))
                                        .ok_or(BuildCredentialError(
                                            "Failed to enter the @context into the credential".to_string(),
                                        ))?;

                                    credential_data
                                        .insert_if_none(&["name"], json!(credential_name))
                                        .ok_or(BuildCredentialError(
                                            "Failed to enter the name into the credential".to_string(),
                                        ))?;

                                    // Validate credential before building
                                    CredentialType::VerifiableCredential
                                        .validate(&credential_data)
                                        .map_err(|e| BuildCredentialError(e.to_string()))?;

                                    return Ok(vec![UnsignedCredentialCreated {
                                        credential_id,
                                        data: Data { raw: credential_data },
                                        credential_configuration,
                                        notification_id: Some(notification_id),
                                        credential_status,
                                        created_at: Some(created_at),
                                        expires_at,
                                    }]);
                                }
                                "AchievementCredential" | "OpenBadgeCredential" => {
                                    credential_data
                                        .insert_if_none(
                                            &["@context"],
                                            json!([
                                                "https://www.w3.org/ns/credentials/v2",
                                                "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.3.json"
                                            ]),
                                        )
                                        .ok_or(BuildCredentialError(
                                            "Failed to enter the @context into the credential".to_string(),
                                        ))?;

                                    credential_data
                                        .insert_at_path(&["issuer", "type"], json!("Profile"))
                                        .ok_or(BuildCredentialError(
                                            "Failed to enter the issuer.type into the credential".to_string(),
                                        ))?;
                                    if let Some(credential_name) = credential_name {
                                        credential_data.insert_if_none(&["name"], json!(credential_name));
                                    } else {
                                        credential_data
                                            .insert_if_none(&["name"], json!("OpenBadge Credential"))
                                            .ok_or(BuildCredentialError(
                                                "Failed to enter the name into the credential".to_string(),
                                            ))?;
                                    }

                                    // Validate credential before building
                                    CredentialType::OpenBadgeCredential
                                        .validate(&credential_data)
                                        .map_err(|e| BuildCredentialError(e.to_string()))?;

                                    return Ok(vec![UnsignedCredentialCreated {
                                        credential_id,
                                        notification_id: Some(notification_id),
                                        data: Data { raw: credential_data },
                                        credential_configuration,
                                        credential_status,
                                        created_at: Some(created_at),
                                        expires_at,
                                    }]);
                                }
                                "EuropeanDigitalCredential" => {
                                    // Currently the ELM schema still references VC DM 1.1.
                                    // It seems like they will be moving to VC DM 2.0. but for now we need to be compatible with both.
                                    // TODO: remove once the ELM schema has been updated to VC DM 2.0.
                                    {
                                        credential_data
                                            .insert_if_none(
                                                &["@context"],
                                                json!(["https://www.w3.org/2018/credentials/v1"]),
                                            )
                                            .ok_or(BuildCredentialError(
                                                "Failed to enter the @context into the credential".to_string(),
                                            ))?;
                                        credential_data
                                            .insert_if_none(
                                                &["issuanceDate"],
                                                json!(created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                                            )
                                            .ok_or(BuildCredentialError(
                                                "Failed to enter the issuanceDate date into the credential".to_string(),
                                            ))?;
                                        if let Some(expiration_date) = expires_at {
                                            credential_data
                                                .insert_at_path(&["expirationDate"], json!(expiration_date))
                                                .ok_or(BuildCredentialError(
                                                    "Failed to enter the expirationDate date into the credential"
                                                        .to_string(),
                                                ))?;
                                        }
                                    }

                                    // No fields in credentialProfiles are actually required by the ELM schema
                                    // TODO: enter empty credentialProfile
                                    credential_data
                                        .insert_at_path(
                                            &["credentialProfiles"],
                                            json!({
                                                "id":"http://data.europa.eu/snb/credential/bdc47cb449",
                                                "type":"Concept",
                                                "inScheme":{
                                                    "id":"http://data.europa.eu/snb/credential/25831c2",
                                                    "type": "ConceptScheme"
                                                }
                                            }),
                                        )
                                        .ok_or(BuildCredentialError(
                                            "Failed to enter the credentialProfiles into the credential".to_string(),
                                        ))?;

                                    // TODO: this is currently hard coded, it can remain so until the use of this property (and all of ELM) becomes more clear and it has purpose to the user
                                    credential_data
                                        .insert_at_path(
                                            &["displayParameter"],
                                            json!({
                                                "id": "urn:epass:displayParameter:1",
                                                "type": "DisplayParameter",
                                                "title": {
                                                    "en": credential_name
                                                },
                                                "inScheme":{
                                                    "id":"http://data.europa.eu/snb/credential/25831c2",
                                                    "type": "ConceptScheme"
                                                }
                                            }),
                                        )
                                        .ok_or(BuildCredentialError(
                                            "Failed to enter the displayParameter into the credential".to_string(),
                                        ))?;

                                    //     .schema(Schema::new(
                                    //         identity_core::common::Url::parse(
                                    //             // FIXME
                                    //             "https://eudiw.org/credentials/schemas/EuropeanDigitalCredentialV3_3.json",
                                    //         )
                                    //         .unwrap(),
                                    //         vec!["JsonSchema".to_string()],
                                    //     ));

                                    // Validate credential before building
                                    // CredentialType::EuropeanDigitalCredential.validate(&credential_data).map_err(|e| BuildCredentialError(e.to_string()))?;

                                    let result = CredentialType::EuropeanDigitalCredential.validate(&credential_data);
                                    if let Err(errors) = result {
                                        println!("Validation errors: {errors:?}");
                                    } else {
                                        println!(
                                            "Credential is valid according to the EuropeanDigitalCredential schema."
                                        );
                                    }

                                    println!("Validation complete.");

                                    return Ok(vec![UnsignedCredentialCreated {
                                        credential_id,
                                        notification_id: Some(notification_id),
                                        data: Data { raw: credential_data },
                                        credential_configuration,
                                        credential_status,
                                        created_at: Some(created_at),
                                        expires_at,
                                    }]);
                                }
                                _ => continue,
                            }
                        }

                        Err(UnsupportedCredentialType)
                    }
                    CredentialFormats::DcSdJwt(Parameters::<DcSdJwt> {
                        parameters: DcSdJwtParameters { vct },
                    }) => {
                        let mut raw = data.raw;
                        raw["vct"] = json!(vct);

                        return Ok(vec![UnsignedCredentialCreated {
                            credential_id,
                            notification_id: Some(notification_id),
                            data: Data { raw },
                            credential_configuration,
                            credential_status,
                            created_at: Some(created_at),
                            expires_at,
                        }]);
                    }
                    CredentialFormats::VcSdJwt(Parameters::<VcSdJwt> {
                        parameters:
                            VcSdJwtParameters {
                                credential_definition: vc_sd_jwt::CredentialDefinition { type_, .. },
                                ..
                            },
                    }) => {
                        return Ok(vec![UnsignedCredentialCreated {
                            credential_id,
                            notification_id: Some(notification_id),
                            data: Data { raw: credential_data },
                            credential_configuration,
                            credential_status,
                            created_at: Some(created_at),
                            expires_at,
                        }]);
                    }
                    _ => Err(UnsupportedCredentialFormat(serde_json::json!(
                        credential_configuration.credential_format
                    ))),
                }
            }

            CreateSignedCredential {
                credential_id,
                signed_credential,
            } => {
                #[cfg(feature = "test_utils")]
                let notification_id = test_utils::notification_id();
                #[cfg(not(feature = "test_utils"))]
                let notification_id = agent_shared::generate_random_string();

                Ok(vec![SignedCredentialCreated {
                    credential_id,
                    signed_credential,
                    notification_id: Some(notification_id),
                }])
            }
            SignCredential {
                credential_id,
                subject_id,
                overwrite,
                proof,
            } => {
                if self.signed.is_some() && !overwrite {
                    return Ok(vec![]);
                }

                #[cfg(feature = "test_utils")]
                let issuance_date = "2010-01-01T00:00:00Z".to_string();
                #[cfg(not(feature = "test_utils"))]
                let issuance_date = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

                let issuance_date =
                    identity_core::common::Timestamp::parse(&issuance_date).expect("Could not parse issuance_date");

                let id: Option<Url> = self
                    .data
                    .as_ref()
                    .and_then(|data| data.raw.get("id"))
                    .and_then(|id| id.as_str())
                    .and_then(|id| Url::parse(id).ok());

                let default_did_method = get_preferred_did_method();

                let issuer_did: identity_core::common::Url = services
                    .issuer
                    .identifier(&default_did_method.to_string(), get_preferred_signing_algorithm())
                    .await
                    .ok()
                    .and_then(|did| did.parse().ok())
                    .ok_or(InvalidIssuerDidError)?;

                let mut credential = self.data.as_ref().ok_or(InvalidCredentialDataError)?.clone();

                let status_list_url = get_status_list_url(self.credential_status.index)?;

                let status_claim = sd_jwt_vc::Status(StatusMechanism::StatusList(StatusListRef {
                    idx: self.credential_status.index,
                    uri: status_list_url,
                }));

                #[cfg(feature = "test_utils")]
                let iat = 1262304000; // 2010-01-01T00:00:00Z
                #[cfg(not(feature = "test_utils"))]
                let iat = issuance_date.to_unix();

                let signed_credential = match &self.credential_configuration.credential_format {
                    CredentialFormats::JwtVcJson(_) => {
                        if let Some(ref id) = id {
                            credential.raw["id"] = json!(id);
                        };

                        let exp = self.expires_at.map(|exp| exp.timestamp());

                        credential.raw["issuer"] = json!(issuer_did);

                        let credential_subject = credential.raw["credentialSubject"].as_object().unwrap().clone();

                        // Create a new Map and insert the id field first
                        let mut new_credential_subject = serde_json::Map::new();

                        if let Some(subject_id) = &subject_id {
                            new_credential_subject.insert("id".to_string(), json!(subject_id));
                        }

                        // Insert the rest of the fields
                        for (key, value) in credential_subject {
                            if key != "id" {
                                new_credential_subject.insert(key, value);
                            }
                        }

                        info!("Credential subject: {:?}", new_credential_subject);

                        // Replace the original credentialSubject with the new map
                        credential.raw["credentialSubject"] = serde_json::Value::Object(new_credential_subject);

                        info!("Credential: {:?}", credential);

                        // Add standard claims
                        let mut vc_jwt_builder = VerifiableCredentialJwt::builder()
                            .iss(issuer_did.to_string())
                            .iat(iat)
                            .nbf(iat); // TODO: setting the `nbf` to `iat` makes the JWT immediately usable

                        if let Some(subject_id) = subject_id {
                            vc_jwt_builder = vc_jwt_builder.sub(subject_id);
                        }

                        let vc_jwt_builder = if let Some(exp) = exp {
                            vc_jwt_builder.exp(exp)
                        } else {
                            vc_jwt_builder
                        };

                        let vc_jwt_builder = if let Some(id) = id {
                            vc_jwt_builder.jti(id.to_string())
                        } else {
                            vc_jwt_builder
                        };

                        let vc_jwt_built = vc_jwt_builder
                            .verifiable_credential(credential.raw)
                            .build()
                            .map_err(|e| CredentialError::BuildCredentialError(e.to_string()))?;

                        let mut vc_jwt_value = serde_json::to_value(&vc_jwt_built)
                            .map_err(|e| CredentialError::BuildCredentialError(e.to_string()))?;

                        let mut vc_jwt_object = vc_jwt_value
                            .as_object_mut()
                            .ok_or(CredentialError::BuildCredentialError(
                                "Failed to convert VC JWT to mutable JSON object".to_string(),
                            ))?
                            .clone();

                        vc_jwt_object.insert("status".to_string(), json!(status_claim));

                        json!(jwt::encode(
                            services.issuer.clone(),
                            Header::new(get_preferred_signing_algorithm()),
                            vc_jwt_object,
                            &default_did_method.to_string()
                        )
                        .await
                        .ok())
                    }
                    CredentialFormats::DcSdJwt(_) => {
                        let issuer = &services.issuer;

                        let algorithm = get_preferred_signing_algorithm();

                        let alg = algorithm.as_str();

                        let holder_kid = proof.and_then(|proof| {
                            let Proof::Jwt { jwt: proof } = proof;
                            jsonwebtoken::decode_header(&proof).ok().and_then(|header| header.kid)
                        });

                        let kid = issuer
                            .key_id(&get_preferred_did_method().to_string(), algorithm)
                            .await
                            .ok_or(KeyIdError)?;

                        let sd_jwt_vc_claims = SdJwtVcClaims::from_json_value(credential.raw.clone())
                            .map_err(|e| BuildCredentialError(format!("Failed to extract SD-JWT VC claims: {}", e)))?;

                        let paths = sd_jwt_vc_claims.keys().cloned().collect::<Vec<String>>();

                        let mut builder = SdJwtVcBuilder::new(credential.raw.clone())
                            .map_err(|e| BuildCredentialError(format!("Failed to create SD-JWT VC builder: {}", e)))?
                            .header("typ", "dc+sd-jwt")
                            .header("kid", kid);

                        builder = builder.iss(issuer_did);
                        builder = builder.status(status_claim);

                        if let Some(holder_kid) = holder_kid {
                            builder = builder.require_key_binding(RequiredKeyBinding::Kid(holder_kid));
                        }

                        builder = builder.iat(issuance_date);
                        builder = builder.nbf(issuance_date);

                        if let Some(expiration_date) = self.expires_at {
                            // tmp
                            builder = builder.exp(
                                identity_core::common::Timestamp::parse(&expiration_date.to_rfc3339())
                                    .expect("Could not parse issuance_date"),
                            );
                        }

                        // By default, all custom claims are concealable.
                        for path in paths {
                            builder = builder.make_concealable(&format!("/{}", path)).map_err(|e| {
                                BuildCredentialError(format!(
                                    "Failed to make claim at path `/{}` concealable: {}",
                                    path, e
                                ))
                            })?;
                        }

                        let sd_jwt_credential = builder
                            .finish(&**issuer, alg)
                            .await
                            .map_err(|e| BuildCredentialError(format!("Failed to build SD-JWT credential: {}", e)))?;

                        serde_json::json!(sd_jwt_credential.to_string())
                    }
                    CredentialFormats::VcSdJwt(_) => {
                        let issuer = &services.issuer;

                        let algorithm = get_preferred_signing_algorithm();

                        let alg = algorithm.as_str();

                        let holder_kid = proof.and_then(|proof| {
                            let Proof::Jwt { jwt: proof } = proof;

                            jsonwebtoken::decode_header(&proof).ok().and_then(|header| header.kid)
                        });

                        let kid = issuer
                            .key_id(&get_preferred_did_method().to_string(), algorithm)
                            .await
                            .ok_or(KeyIdError)?;

                        let mut w3c_verifiable_credential_v2: W3CVerifiableCredentialV2 =
                            W3CVerifiableCredentialV2::from_json_value(credential.raw).map_err(|e| {
                                BuildCredentialError(format!(
                                    "Failed to extract W3C Verifiable Credential V2 claims: {}",
                                    e
                                ))
                            })?;

                        w3c_verifiable_credential_v2.valid_from = issuance_date;

                        if let Some(expiration_date) = self.expires_at {
                            w3c_verifiable_credential_v2.valid_until = Some(
                                identity_core::common::Timestamp::parse(&expiration_date.to_rfc3339())
                                    .expect("Could not parse issuance_date"),
                            );
                        }

                        let paths = w3c_verifiable_credential_v2
                            .credential_subject
                            .first()
                            .map(|subject| subject.properties.keys().cloned().collect::<Vec<String>>())
                            .unwrap_or_default();

                        // TODO: If necessary, convert `w3c_verifiable_credential_v2` to a serde_json::Value.

                        let mut builder = SdJwtBuilder::new(w3c_verifiable_credential_v2)
                            .map_err(|e| BuildCredentialError(format!("Failed to create SD-JWT VC builder: {}", e)))?
                            .header("typ", "vc+sd-jwt")
                            .header("kid", kid)
                            .insert_claim("status", status_claim)
                            .map_err(|e| BuildCredentialError(format!("Failed to create SD-JWT VC builder: {}", e)))?;

                        if let Some(holder_kid) = holder_kid.clone() {
                            builder = builder.require_key_binding(RequiredKeyBinding::Kid(holder_kid));
                        }

                        // By default, all custom claims are concealable.
                        for path in paths {
                            builder = builder
                                .make_concealable(&format!("/credentialSubject/{}", path))
                                .map_err(|e| {
                                    BuildCredentialError(format!(
                                        "Failed to make claim at path `/credentialSubject/{}` concealable: {}",
                                        path, e
                                    ))
                                })?;
                        }

                        let vc_sd_jwt_credential = builder
                            .finish(&**issuer, alg)
                            .await
                            .map_err(|e| BuildCredentialError(format!("Failed to build SD-JWT credential: {}", e)))?;

                        serde_json::json!(vc_sd_jwt_credential.to_string())
                    }
                    _ => {
                        return Err(UnsupportedCredentialFormat(serde_json::json!(
                            self.credential_configuration.credential_format
                        )));
                    }
                };

                Ok(vec![CredentialSigned {
                    credential_id,
                    signed_credential,
                    status: Status::Issued,
                }])
            }
            AddNotification {
                credential_id,
                notification,
            } => Ok(vec![CredentialEvent::NotificationReceived {
                credential_id,
                notification,
            }]),
            UpdateCredentialStatus {
                credential_id,
                credential_status,
            } => Ok(vec![CredentialEvent::CredentialStatusUpdated {
                credential_id,
                credential_status,
            }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            UnsignedCredentialCreated {
                credential_id,
                data,
                credential_configuration,
                notification_id,
                credential_status,
                created_at,
                expires_at,
            } => {
                self.credential_id = credential_id;
                self.data.replace(data);
                self.credential_configuration = *credential_configuration;
                self.notification_id = notification_id;
                self.credential_status = credential_status;
                self.created_at = created_at;
                self.expires_at = expires_at;
            }
            SignedCredentialCreated {
                credential_id,
                signed_credential,
                notification_id,
            } => {
                self.credential_id = credential_id;
                self.signed.replace(signed_credential);
                self.notification_id = notification_id;
            }
            CredentialSigned {
                credential_id,
                signed_credential,
                status,
            } => {
                self.credential_id = credential_id;
                self.signed.replace(signed_credential);
                self.status = status;
            }
            NotificationReceived {
                credential_id,
                notification,
            } => {
                self.credential_id = credential_id;
                self.holder_notifications.push(notification);
            }
            CredentialStatusUpdated {
                credential_id,
                credential_status,
            } => {
                self.credential_id = credential_id;
                self.credential_status = credential_status;
            }
        }
    }
}

// Helpers

// Helper methods to simplify working with serde_json::Value.
pub trait ExtraMethods {
    /// Inserts a value at the specified path, creating intermediate objects as needed.
    /// The path includes the final key name where the value will be inserted.
    /// For example, to set `$.issuer.id = "123"`, use:
    /// `credential.add_value_or_insert(&["issuer", "id"], json!("123"))`
    ///
    /// Returns `Some(&mut self)` on success, `None` on failure.
    fn insert_at_path(&mut self, path: &[&str], value: serde_json::Value) -> Option<&mut Self>;
    fn insert_if_none(&mut self, path: &[&str], value: serde_json::Value) -> Option<&mut Self>;
}

impl ExtraMethods for serde_json::Value {
    fn insert_at_path(&mut self, path: &[&str], value: serde_json::Value) -> Option<&mut Self> {
        let (last_key, parent_path) = path.split_last()?;

        let mut current_value: &mut Value = self;

        // Navigate/create path to parent of final key
        for key in parent_path {
            current_value = current_value
                // TODO: add array handling here too?
                .as_object_mut()?
                .entry((*key).to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        }

        // Insert the value at the final key
        current_value.as_object_mut()?.insert(last_key.to_string(), value);

        Some(self)
    }

    fn insert_if_none(&mut self, path: &[&str], value: serde_json::Value) -> Option<&mut Self> {
        let (last_key, parent_path) = path.split_last()?;

        let mut current_value: &mut Value = self;

        // Navigate to parent of final key
        for key in parent_path {
            current_value = current_value.as_object_mut()?.get_mut(*key)?;
        }

        // Insert the value at the final key if it doesn't exist
        current_value
            .as_object_mut()?
            .entry(last_key.to_string())
            .or_insert(value);

        Some(self)
    }
}

fn get_status_list_url(index: usize) -> Result<identity_core::common::Url, CredentialError> {
    let statuses_per_byte: usize = 8 / BITS_PER_STATUS as usize;
    let status_list_number = index / ((STATUS_LIST_BYTES_AMOUNT * statuses_per_byte) as f64 * 0.7) as usize;

    let mut status_list_url = config().ietf_oauth_token_status_list_uri.clone();
    status_list_url
        .path_segments_mut()
        .map_err(|_| CredentialError::InvalidCredentialStatus)?
        .push(&status_list_number.to_string());

    Ok(status_list_url.into())
}

#[cfg(test)]
pub mod credential_tests {
    use super::test_utils::*;
    use super::*;

    use jsonwebtoken::Algorithm;

    use rstest::rstest;
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::credential::aggregate::Credential;
    use crate::credential::event::CredentialEvent;
    use crate::offer::aggregate::test_utils::holder;
    use agent_secret_manager::service::Service;
    use oid4vc_core::Subject;

    type CredentialTestFramework = TestFramework<Credential>;

    #[rstest]
    #[case::openbadges(
        OPENBADGE_CREDENTIAL_SUBJECT.clone(),
        OPENBADGE_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_OPENBADGE_CREDENTIAL.clone()
    )]
    #[case::w3c_vc(
        W3C_VC_CREDENTIAL_SUBJECT.clone(),
        W3C_VC_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_W3C_VC_CREDENTIAL.clone()
    )]
    #[case::dc_sd_jwt(
        DC_SD_JWT_CREDENTIAL_SUBJECT.clone(),
        DC_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_DC_SD_JWT_CREDENTIAL.clone()
    )]
    #[case::vc_sd_jwt(
        VC_SD_JWT_CREDENTIAL_SUBJECT.clone(),
        VC_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_VC_SD_JWT_CREDENTIAL.clone()
    )]
    #[serial_test::serial]
    async fn test_create_unsigned_credential(
        #[case] credential_subject: serde_json::Value,
        #[case] credential_configuration: CredentialConfigurationsSupportedObject,
        #[case] unsigned_credential: serde_json::Value,
        credential_id: String,
        notification_id: String,
        created_at: DateTime<Utc>,
    ) {
        CredentialTestFramework::with(IssuanceServices::default().await)
            .given_no_previous_events()
            .when(CredentialCommand::CreateUnsignedCredential {
                credential_id: credential_id.clone(),
                data: Data {
                    raw: credential_subject,
                },
                credential_configuration: Box::new(credential_configuration.clone()),
                expires_at: CredentialExpiry::Never,
                credential_status_index: 0,
            })
            .then_expect_events(vec![CredentialEvent::UnsignedCredentialCreated {
                credential_id,
                data: Data {
                    raw: unsigned_credential,
                },
                notification_id: Some(notification_id.clone()),
                credential_configuration: Box::new(credential_configuration),
                credential_status: CredentialStatus {
                    index: 0,
                    status: StatusType::VALID,
                },
                created_at: Some(created_at),
                expires_at: None,
            }])
    }

    #[rstest]
    #[case::openbadges(
        UNSIGNED_OPENBADGE_CREDENTIAL.clone(),
        OPENBADGE_CREDENTIAL_CONFIGURATION.clone(),
        OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string(),
    )]
    #[case::w3c_vc(
        UNSIGNED_W3C_VC_CREDENTIAL.clone(),
        W3C_VC_CREDENTIAL_CONFIGURATION.clone(),
        W3C_VC_VERIFIABLE_CREDENTIAL_JWT.to_string(),
    )]
    #[serial_test::serial]
    async fn test_sign_credential(
        #[future(awt)] holder: Arc<dyn Subject>,
        #[case] unsigned_credential: serde_json::Value,
        #[case] credential_configuration: CredentialConfigurationsSupportedObject,
        #[case] verifiable_credential_jwt: String,
        credential_id: String,
        created_at: DateTime<Utc>,
    ) {
        CredentialTestFramework::with(IssuanceServices::default().await)
            .given(vec![CredentialEvent::UnsignedCredentialCreated {
                credential_id: credential_id.clone(),
                data: Data {
                    raw: unsigned_credential,
                },
                credential_configuration: Box::new(credential_configuration),
                notification_id: None,
                credential_status: CredentialStatus {
                    index: 0,
                    status: StatusType::VALID,
                },
                created_at: Some(created_at),
                expires_at: None,
            }])
            .when(CredentialCommand::SignCredential {
                credential_id: credential_id.clone(),
                subject_id: Some(holder.identifier("did:key", Algorithm::EdDSA).await.unwrap()),
                overwrite: false,
                proof: None,
            })
            .then_expect_events(vec![CredentialEvent::CredentialSigned {
                credential_id,
                signed_credential: json!(verifiable_credential_jwt),
                status: Status::Issued,
            }])
    }

    pub mod expiry_tests {
        use super::*;

        #[test]
        fn custom_serializer_for_credential_expiry() {
            let deserialized: CredentialExpiry = serde_json::from_value(serde_json::json!("never")).unwrap();
            assert_eq!(deserialized, CredentialExpiry::Never);

            let serialized = serde_json::to_value(&CredentialExpiry::Never).unwrap();
            assert_eq!(serialized, serde_json::json!("never"));
        }
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use lazy_static::lazy_static;
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
        },
        credential_issuer::credential_configurations_supported::{CredentialConfigurationsSupportedDisplay, Logo},
        proof::{KeyProofMetadata, ProofType},
    };
    use rstest::fixture;
    use serde_json::json;
    use std::collections::HashMap;

    #[fixture]
    pub fn notification_id() -> String {
        "notification_id".to_string()
    }

    #[fixture]
    pub fn created_at() -> chrono::DateTime<chrono::Utc> {
        "2010-01-01T00:00:00Z".parse().unwrap()
    }

    pub const OPENBADGE_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6Imh0dHBzOi8vZXhhbXBsZS5jb20vY3JlZGVudGlhbHMvMzUyNyIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3AwL2NvbnRleHQtMy4wLjMuanNvbiJdLCJpZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vY3JlZGVudGlhbHMvMzUyNyIsInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJPcGVuQmFkZ2VDcmVkZW50aWFsIl0sImlzc3VlciI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiaXNzdWFuY2VEYXRlIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJuYW1lIjoiVGVhbXdvcmsgQmFkZ2UiLCJjcmVkZW50aWFsU3ViamVjdCI6eyJpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwidHlwZSI6WyJBY2hpZXZlbWVudFN1YmplY3QiXSwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbGUuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEiOnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY29nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNvZ25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbnQuIiwibmFtZSI6IlRlYW13b3JrIn19LCJjcmVkZW50aWFsU3RhdHVzIjp7ImlkIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwidHlwZSI6InN0YXR1c2xpc3Qrand0IiwidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjowfX0sInN0YXR1cyI6eyJzdGF0dXNfbGlzdCI6eyJ1cmkiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJpZHgiOjB9fX0.PDwoMAawtjYr-cn5tfcPpnatf8cLuJMtaGXwsmEGimE-ki_fS8B1itBMGeQZyPhqhJIpD7ZepxYEn7rMXc0fDg";

    pub const W3C_VC_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIl0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiXSwiY3JlZGVudGlhbFN1YmplY3QiOnsiaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4iLCJkZWdyZWUiOnsidHlwZSI6Ik1hc3RlckRlZ3JlZSIsIm5hbWUiOiJNYXN0ZXIgb2YgT2NlYW5vZ3JhcGh5In19LCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsImlzc3VhbmNlRGF0ZSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwiY3JlZGVudGlhbFN0YXR1cyI6eyJpZCI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsInR5cGUiOiJzdGF0dXNsaXN0K2p3dCIsInVyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsImlkeCI6MH19LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjowfX19.O95yvZmczmZs7-crtkthFgF2YNRHsfaiBfWPe-aL9flxoq-upcpfR2NsvvK5t_EojWgeXICL4XY358HCr_ADCA";

    #[fixture]
    pub fn credential_id() -> String {
        "credential_id".to_string()
    }

    lazy_static! {
        pub static ref OPENBADGE_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    parameters: (CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                        credential_subject: Default::default(),
                    })
                    .into(),
                }),
                cryptographic_binding_methods_supported: vec!["did:key".to_string(), "did:jwk".to_string(),],
                credential_signing_alg_values_supported: vec!["EdDSA".to_string()],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec!["EdDSA".to_string()],
                    },
                )]),
                display: vec![CredentialConfigurationsSupportedDisplay {
                    name: "Teamwork Badge".to_string(),
                    locale: None,
                    logo: Some(Logo {
                        uri: "https://www.impierce.com/external/impierce-logo.png".parse().unwrap(),
                        alt_text: Some("Impierce Logo".to_string()),
                    }),
                    description: None,
                    background_image: None,
                    background_color: None,
                    text_color: None,
                }],
                ..Default::default()
            };
        pub static ref W3C_VC_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    parameters: (CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string()],
                        credential_subject: Default::default(),
                    })
                    .into()
                }),
                cryptographic_binding_methods_supported: vec!["did:jwk".to_string(), "did:key".to_string(),],
                credential_signing_alg_values_supported: vec!["ES256".to_string(), "EdDSA".to_string()],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec!["ES256".to_string(), "EdDSA".to_string()],
                    },
                )]),
                display: vec![CredentialConfigurationsSupportedDisplay {
                    name: "Verifiable Credential".to_string(),
                    locale: Some("en".to_string()),
                    logo: Some(Logo {
                        uri: "https://www.impierce.com/external/impierce-logo.png".parse().unwrap(),
                        alt_text: Some("Impierce Logo".to_string()),
                    }),
                    description: None,
                    background_image: None,
                    background_color: None,
                    text_color: None,
                }],
                ..Default::default()
            };
        pub static ref DC_SD_JWT_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::DcSdJwt(Parameters {
                    parameters: ("http://localhost:3033/vct/U0QtSldU/0".to_string()).into()
                }),
                cryptographic_binding_methods_supported: vec!["did:jwk".to_string(), "did:key".to_string(),],
                credential_signing_alg_values_supported: vec!["ES256".to_string(), "EdDSA".to_string()],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec!["ES256".to_string(), "EdDSA".to_string()],
                    },
                )]),
                display: vec![CredentialConfigurationsSupportedDisplay {
                    name: "SD-JWT Credential".to_string(),
                    locale: Some("en".to_string()),
                    logo: Some(Logo {
                        uri: "https://www.impierce.com/external/impierce-logo.png".parse().unwrap(),
                        alt_text: Some("Impierce Logo".to_string()),
                    }),
                    description: None,
                    background_image: None,
                    background_color: None,
                    text_color: None,
                }],
                ..Default::default()
            };
        pub static ref VC_SD_JWT_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::VcSdJwt(Parameters {
                    parameters: (vc_sd_jwt::CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string()],
                    })
                    .into()
                }),
                cryptographic_binding_methods_supported: vec!["did:jwk".to_string(), "did:key".to_string(),],
                credential_signing_alg_values_supported: vec!["ES256".to_string(), "EdDSA".to_string()],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec!["ES256".to_string(), "EdDSA".to_string()],
                    },
                )]),
                display: vec![CredentialConfigurationsSupportedDisplay {
                    name: "VCDM2.0 SD-JWT Credential".to_string(),
                    locale: Some("en".to_string()),
                    logo: Some(Logo {
                        uri: "https://www.impierce.com/external/impierce-logo.png".parse().unwrap(),
                        alt_text: Some("Impierce Logo".to_string()),
                    }),
                    description: None,
                    background_image: None,
                    background_color: None,
                    text_color: None,
                }],
                ..Default::default()
            };
        pub static ref OPENBADGE_CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "id": "https://example.com/credentials/3527",
                "credentialSubject": {
                    "type": [ "AchievementSubject" ],
                    "achievement": {
                              "id": "https://example.com/achievements/21st-century-skills/teamwork",
                              "type": "Achievement",
                              "criteria": {
                                  "narrative": "Team members are nominated for this badge by their peers and recognized upon review by Example Corp management."
                              },
                              "description": "This badge recognizes the development of the capacity to collaborate within a group environment.",
                              "name": "Teamwork"
                          }
                  }
            }
        );
        pub static ref W3C_VC_CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "credentialSubject": {
                    "first_name": "Ferris",
                    "last_name": "Rustacean",
                    "degree": {
                        "type": "MasterDegree",
                        "name": "Master of Oceanography"
                    }
                }
            }
        );
        pub static ref DC_SD_JWT_CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "first_name": "Ferris",
                "last_name": "Rustacean"
            }
        );
        pub static ref VC_SD_JWT_CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "credentialSubject": {
                    "first_name": "Ferris",
                    "last_name": "Rustacean",
                    "degree": {
                        "type": "MasterDegree",
                        "name": "Master of Oceanography"
                    }
                }
            }
        );
        pub static ref UNSIGNED_OPENBADGE_CREDENTIAL: serde_json::Value = json!({
          "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.3.json"
          ],
          "id": "https://example.com/credentials/3527",
          "type": ["VerifiableCredential", "OpenBadgeCredential"],
          "issuer": {
            "id": "https://my-domain.example.org/",
            "type": "Profile",
            "name": "UniCore"
          },
          "issuanceDate": "2010-01-01T00:00:00Z",
          "name": "Teamwork Badge",
          "credentialSubject": OPENBADGE_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
          "credentialStatus": {
              "id": "https://my-domain.example.org/ietf-oauth-token-status-list/0",
              "type": "statuslist+jwt",
              "uri": "https://my-domain.example.org/ietf-oauth-token-status-list/0",
              "idx": 0
          }
        });
        pub static ref UNSIGNED_W3C_VC_CREDENTIAL: serde_json::Value = json!({
          "@context": [ "https://www.w3.org/2018/credentials/v1" ],
          "type": [ "VerifiableCredential" ],
          "credentialSubject": W3C_VC_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
          "issuer": {
            "id": "https://my-domain.example.org/",
            "name": "UniCore"
          },
          "issuanceDate": "2010-01-01T00:00:00Z",
          "credentialStatus": {
              "id": "https://my-domain.example.org/ietf-oauth-token-status-list/0",
              "type": "statuslist+jwt",
              "uri": "https://my-domain.example.org/ietf-oauth-token-status-list/0",
              "idx": 0
          }
        });
        pub static ref UNSIGNED_DC_SD_JWT_CREDENTIAL: serde_json::Value = json!({
            "vct": "http://localhost:3033/vct/U0QtSldU/0",
            "first_name": "Ferris",
            "last_name": "Rustacean"
        });
        pub static ref UNSIGNED_VC_SD_JWT_CREDENTIAL: serde_json::Value = json!({
          "@context": [ "https://www.w3.org/ns/credentials/v2" ],
          "type": [ "VerifiableCredential" ],
          "credentialSubject": VC_SD_JWT_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
          "issuer": {
            "id": "https://my-domain.example.org/",
            "name": "UniCore"
          },
          "validFrom": "2010-01-01T00:00:00Z",
          "credentialStatus": {
              "id": "https://my-domain.example.org/ietf-oauth-token-status-list/0",
              "type": "statuslist+jwt",
              "uri": "https://my-domain.example.org/ietf-oauth-token-status-list/0",
              "idx": 0
          }
        });
    }
}
