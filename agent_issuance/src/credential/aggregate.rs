use super::entity::Data;
use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self, *};
use crate::credential::event::CredentialEvent;
use crate::services::IssuanceServices;
use agent_library::json_schema::CredentialType;
use agent_shared::config::{
    config, get_preferred_did_method, get_preferred_signing_algorithm, AlgorithmExt, BITS_PER_STATUS,
    STATUS_LIST_BYTES_AMOUNT,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
use identity_core::common::Timestamp;
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
use oid4vci::VerifiableCredentialJwt;
use sd_jwt::{RequiredKeyBinding, SdJwtBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, info};
use url::Url;

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

                // created_at is only used for the `CreatedUnsignedCredential` event and is not equal to the issuanceDate(VC1.1), issued (ELM) nor validFrom (VC2 & OBv3) fields.
                // These are only added during signing (SignCredential). (issuanceDate and issued will be overwritten even if provided in the payload, validFrom will not).
                #[cfg(feature = "test_utils")]
                let created_at: DateTime<Utc> = "2010-01-01T00:00:00Z"
                    .parse()
                    .map_err(|e| BuildCredentialError(format!("Failed to parse created_at: {}", e)))?;
                #[cfg(not(feature = "test_utils"))]
                let created_at: DateTime<Utc> = chrono::Utc::now();

                let expires_at = match expires_at {
                    CredentialExpiry::Fixed(fixed) => Some(fixed),
                    CredentialExpiry::Never => None,
                };

                let credential_status = CredentialStatus {
                    index: credential_status_index,
                    status: StatusType::VALID,
                };

                let mut credential_data = data.raw.clone();

                match &credential_configuration.credential_format {
                    CredentialFormats::JwtVcJson(Parameters::<JwtVcJson> {
                        parameters:
                            JwtVcJsonParameters {
                                credential_definition: CredentialDefinition { type_, .. },
                                ..
                            },
                    }) => {
                        // Set the type to the original credential configuration type. // TODO: More enforcement that the credential adheres to the credential_configuration which has been offered to the receiving party.
                        credential_data
                            .insert_at_path(&["type"], json!(type_))
                            .ok_or(BuildCredentialError(
                                "Failed to enter the type into the credential".to_string(),
                            ))?;

                        match build_unsigned_credential_data(
                            type_,
                            &mut credential_data,
                            &credential_configuration,
                            expires_at,
                            credential_status_index,
                        ) {
                            Ok(credential_data) => {
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
                            Err(e) => {
                                return Err(BuildCredentialError(e.to_string()));
                            }
                        }
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
                        // Set the type to the original credential configuration type. // TODO: More enforcement that the credential adheres to the credential_configuration which has been offered to the receiving party.
                        credential_data
                            .insert_at_path(&["type"], json!(type_))
                            .ok_or(BuildCredentialError(
                                "Failed to enter the type into the credential".to_string(),
                            ))?;

                        match build_unsigned_credential_data(
                            type_,
                            &mut credential_data,
                            &credential_configuration,
                            expires_at,
                            self.credential_status.index,
                        ) {
                            Ok(credential_data) => {
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
                            Err(e) => {
                                return Err(BuildCredentialError(e.to_string()));
                            }
                        }
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
                proofs,
            } => {
                if self.signed.is_some() && !overwrite {
                    return Ok(vec![]);
                }

                // Create/collect claims needed for the signed (SD-)JWT
                // These claims will be used to populate the last part of the credential data as well, as specs dictate there should be no conflict between the jwt claims and credential data.
                #[cfg(feature = "test_utils")]
                let iat = "2010-01-01T00:00:00Z".to_string();
                #[cfg(not(feature = "test_utils"))]
                let iat = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

                let created_at = iat.clone();
                let iat = identity_core::common::Timestamp::parse(&iat).expect("Could not parse issuance_date");

                // iss is set to the issuer DID
                let default_did_method = get_preferred_did_method();

                let iss: identity_core::common::Url = services
                    .issuer
                    .identifier(&default_did_method.to_string(), get_preferred_signing_algorithm())
                    .await
                    .ok()
                    .and_then(|did| did.parse().ok())
                    .ok_or(InvalidIssuerDidError)?;

                // Set status claim, seems to miss typ field? TODO
                let status_list_url = get_status_list_url(self.credential_status.index)?;

                let status_claim = sd_jwt_vc::Status(StatusMechanism::StatusList(StatusListRef {
                    idx: self.credential_status.index,
                    uri: status_list_url,
                }));

                // The sensible default for the jti is equal to the credential root `id` field
                let jti: Option<Url> = self
                    .data
                    .as_ref()
                    .and_then(|data| data.raw.get("id"))
                    .and_then(|id| id.as_str())
                    .and_then(|id| Url::parse(id).ok());

                // TODO: add this value back to self in the end
                let credential_data = self.data.as_ref().ok_or(InvalidCredentialDataError)?.raw.clone();
                let credential_data = build_signed_credential_data(
                    credential_data,
                    &self.credential_configuration,
                    created_at,
                    iss.to_string(),
                    subject_id.clone(),
                )?;

                info!(
                    "Credential data to be signed (excluding JWT claims): {:?}",
                    credential_data
                );

                // TODO: this should also be used in JwtVcJson right?
                // If proof is provided then set the holder_kid needs to be extracted to set the `cnf` claim. TODO: shouldnt this be set in JwtVcJson as well?
                let holder_kid = proofs.and_then(|proofs| {
                    // TODO: Support batch credential issuance. See https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-batch-credential-issuance
                    jsonwebtoken::decode_header(proofs.jwt.first()?)
                        .ok()
                        .and_then(|header| header.kid)
                });

                // Set the jwt claims through the specific builder and build the JWT which will be signed at the bottom of each match arm
                let signed_credential = match &self.credential_configuration.credential_format {
                    CredentialFormats::JwtVcJson(_) => {
                        let mut vc_jwt_builder = VerifiableCredentialJwt::builder()
                            .iss(iss.to_string())
                            .iat(iat.to_unix())
                            .nbf(iat.to_unix()); // TODO: setting the `nbf` to `iat` makes the JWT immediately usable

                        // TODO: not sure about the specs but since we now completely enfore alignment between jwt claims and credential data, wouldn't it be more straightforward to add all claims to all our jwt formats?

                        // TODO: shouldnt this be in all formats?
                        if let Some(subject_id) = subject_id {
                            vc_jwt_builder = vc_jwt_builder.sub(subject_id);
                        }

                        let vc_jwt_builder = if let Some(exp) = self.expires_at {
                            vc_jwt_builder.exp(exp.timestamp())
                        } else {
                            vc_jwt_builder
                        };

                        let vc_jwt_builder = if let Some(id) = jti {
                            vc_jwt_builder.jti(id.to_string())
                        } else {
                            vc_jwt_builder
                        };

                        let vc_jwt_built = vc_jwt_builder
                            .verifiable_credential(credential_data)
                            .build()
                            .map_err(|e| CredentialError::BuildCredentialError(e.to_string()))?;

                        let mut vc_jwt_value = serde_json::to_value(&vc_jwt_built)
                            .map_err(|e| CredentialError::BuildCredentialError(e.to_string()))?;

                        // Convert the value to a mutable object to insert the status claim which we cannot use from the VerifiableCredentialJwt builder.
                        let mut vc_jwt_object = vc_jwt_value
                            .as_object_mut()
                            .ok_or(CredentialError::BuildCredentialError(
                                "Failed to convert VC JWT to mutable JSON object".to_string(),
                            ))?
                            .clone();

                        vc_jwt_object.insert("status".to_string(), json!(status_claim));

                        // jwt::encode sets the header by itself.
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
                        // Get the kid for the header of the DcSdJwt
                        let issuer = &services.issuer;
                        let algorithm = get_preferred_signing_algorithm();
                        let kid = issuer
                            .key_id(&get_preferred_did_method().to_string(), algorithm)
                            .await
                            .ok_or(KeyIdError)?;

                        let mut builder = SdJwtVcBuilder::new(&credential_data)
                            .map_err(|e| BuildCredentialError(format!("Failed to create SD-JWT VC builder: {}", e)))?
                            .header("typ", "dc+sd-jwt")
                            .header("kid", kid);

                        // Set the JWT claims
                        builder = builder.iss(iss);
                        builder = builder.iat(iat);
                        builder = builder.nbf(iat);
                        builder = builder.status(status_claim);

                        if let Some(expiration_date) = self.expires_at {
                            builder = builder.exp(
                                Timestamp::parse(&expiration_date.to_rfc3339()).expect("Could not parse issuance_date"),
                            );
                        }

                        // This sets the `cnf` claim
                        if let Some(holder_kid) = holder_kid {
                            builder = builder.require_key_binding(RequiredKeyBinding::Kid(holder_kid));
                        }

                        // By default set all custom claims to concealable.
                        // TODO: I dont think this only gets the custom claims?
                        let sd_jwt_vc_claims = SdJwtVcClaims::from_json_value(credential_data.clone())
                            .map_err(|e| BuildCredentialError(format!("Failed to extract SD-JWT VC claims: {}", e)))?;

                        let paths = sd_jwt_vc_claims.keys().cloned().collect::<Vec<String>>();

                        for path in paths {
                            builder = builder.make_concealable(&format!("/{}", path)).map_err(|e| {
                                BuildCredentialError(format!(
                                    "Failed to make claim at path `/{}` concealable: {}",
                                    path, e
                                ))
                            })?;
                        }

                        let sd_jwt_credential = builder
                            .finish(&**issuer, algorithm.as_str())
                            .await
                            .map_err(|e| BuildCredentialError(format!("Failed to build SD-JWT credential: {}", e)))?;

                        serde_json::json!(sd_jwt_credential.to_string())
                    }
                    CredentialFormats::VcSdJwt(_) => {
                        // Get the kid for the header of the VcSdJwt
                        let issuer = &services.issuer;
                        let algorithm = get_preferred_signing_algorithm();
                        let kid = issuer
                            .key_id(&get_preferred_did_method().to_string(), algorithm)
                            .await
                            .ok_or(KeyIdError)?;

                        // TODO: shouldn't claims be set here as well?

                        let mut builder = SdJwtBuilder::new(&credential_data)
                            .map_err(|e| BuildCredentialError(format!("Failed to create SD-JWT VC builder: {}", e)))?
                            .header("typ", "vc+sd-jwt")
                            .header("kid", kid)
                            .insert_claim("status", status_claim)
                            .map_err(|e| BuildCredentialError(format!("Failed to create SD-JWT VC builder: {}", e)))?;

                        // This sets the `cnf` claim
                        if let Some(holder_kid) = holder_kid.clone() {
                            builder = builder.require_key_binding(RequiredKeyBinding::Kid(holder_kid));
                        }

                        // TODO: see todo comment above, adding this line now to check if this fixes VerifiableCredentialRecord::try_new() error in identity-wallet
                        builder = builder
                            .insert_claim("iss", iss)
                            .map_err(|_| BuildCredentialError("Failed to insert 'iss' claim".to_string()))?;

                        // By default set all custom claims to concealable.
                        // TODO: only CredentialSubject again, but perhaps only getting the Cred Subject claims is a good idea after all, I remember something about credentialStatus and RefreshService should never be concealable.
                        let paths = credential_data
                            .get("credentialSubject")
                            .and_then(|c| c.as_object())
                            .ok_or(BuildCredentialError(
                                "Failed to convert credential data to JSON object".to_string(),
                            ))?
                            .keys()
                            .cloned()
                            .collect::<Vec<String>>();
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
                            .finish(&**issuer, algorithm.as_str())
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

fn build_signed_credential_data(
    mut credential_data: serde_json::Value,
    credential_configuration: &CredentialConfigurationsSupportedObject,
    created_at: String,
    iss: String,
    subject_id: Option<String>,
) -> Result<serde_json::Value, CredentialError> {
    let credential_types = credential_data
        .get("type")
        .and_then(|t| t.as_array())
        .ok_or(InvalidCredentialDataError)?
        .iter()
        .filter_map(|t| t.as_str())
        .map(|s| s.to_string())
        .collect::<Vec<String>>();

    // Add validFrom as per VC DM 2.0
    // This means we default to setting the `validFrom` to the creation date of the credential as a sensible default.
    // However it is allowed to not enter any validity period and make an OBv3 credential valid eternally in to the past and/or future.
    // TODO: create a way through which this sensible default can be turned off.
    credential_data
        .insert_if_none(&["validFrom"], json!(created_at))
        .ok_or(BuildCredentialError(
            "Failed to enter the validFrom date into the credential".to_string(),
        ))?;

    credential_data.insert_at_path(&["issuer", "id"], json!(iss));

    if let Some(subject_id) = &subject_id {
        credential_data.insert_at_path(&["credentialSubject", "id"], json!(subject_id));
    }
    // Loop through all the items in the `type` array in reverse until we find a match.
    // This looping assumes the most specific type to match on is the latest one in the array.
    // This is an implicit consequence of the typing rules in digital credential formats.
    // For example, for OBv3 as well as ELM the first type is `VerifiableCredential` and the second type is its own type (e.g. `OpenBadgeCredential`/`EuropeanDigitalCredential`).
    for credential_type in credential_types.iter().rev() {
        match credential_type.as_str() {
            "VerifiableCredential" => {
                // JwtVcJson is still based on VC DM 1.1, while VcSdJwt (vc+sd-jwt) is based on VC DM 2.0.
                if matches!(
                    credential_configuration.credential_format,
                    CredentialFormats::JwtVcJson(_)
                ) {
                    credential_data
                        .insert_if_none(&["issuanceDate"], json!(created_at))
                        .ok_or(BuildCredentialError(
                            "Failed to enter the issuanceDate into the credential".to_string(),
                        ))?;
                }

                // Validate credential data before signing
                CredentialType::VerifiableCredential
                    .validate(&credential_data)
                    .map_err(|e| BuildCredentialError(e.to_string()))?;
            }
            "AchievementCredential" | "OpenBadgeCredential" => {
                // Validate credential before building
                CredentialType::OpenBadgeCredential
                    .validate(&credential_data)
                    .map_err(|e| BuildCredentialError(e.to_string()))?;
            }
            "EuropeanDigitalCredential" => {
                // Due to ELM schema requiring a `validFrom` while still building on VC DM 1.1, we also still need `issuanceDate`.
                credential_data
                    .insert_if_none(&["issuanceDate"], json!(created_at))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the issuanceDate date into the credential".to_string(),
                    ))?;
                credential_data
                    .insert_if_none(&["validFrom"], json!(created_at))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the validFrom date into the credential".to_string(),
                    ))?;

                // The following link explains the difference between `issued`, `issuanceDate` and `validFrom`:
                // https://europa.eu/europass/elm-browser/homepage/3-2-0/edc-generic-no-cv_en.html
                credential_data
                    .insert_if_none(&["issued"], json!(created_at))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the issued date into the credential".to_string(),
                    ))?;

                // Validate credential before building
                CredentialType::EuropeanDigitalCredential
                    .validate(&credential_data)
                    .map_err(|e| BuildCredentialError(e.to_string()))?;
            }
            _ => continue,
        }
    }

    Ok(credential_data.clone())
}

/// This builds the credential according to the last given type in the provided type array which matches with our supported credential types.
/// The first block builds fields common for all our current supported credential types.
/// The match case builds the fields specific to the credential type and validates the credential against its Json Schema before returning it.
/// Every Error is returned as a BuildCredentialError and handled upstream.
/// NOTE: Keep in mind that all data used during signing (SignCredential) for the JWT claims also overwites its Credential Data Model counterparts, this includes:
/// - `issuer.id`
/// - `credentialSubject.id`
/// - `issuanceDate`/`validFrom`/`issued`
/// - `expirationDate`/`validUntil`
fn build_unsigned_credential_data(
    credential_types: &[String],
    credential_data: &mut serde_json::Value,
    credential_configuration: &CredentialConfigurationsSupportedObject,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    credential_status_index: usize,
) -> Result<serde_json::Value, CredentialError> {
    let credential_name = credential_configuration
        .credential_metadata
        .as_ref()
        .and_then(|meta| meta.display.as_ref())
        .and_then(|display| display.first())
        .map(|d| d.name.clone());

    // Add issuer name reflecting the UniCore configuration
    let issuer_name = config()
        .display
        .first()
        .ok_or(BuildCredentialError("Could not find the issuer name".to_string()))?
        .name
        .clone();

    credential_data
        .insert_at_path(&["issuer", "name"], json!(issuer_name))
        .ok_or(BuildCredentialError(
            "Failed to enter the issuer.name into the credential".to_string(),
        ))?;

    // If no root id is provided, set the issuer public url as sensible default.
    credential_data
        .insert_if_none(&["id"], json!(config().public_url))
        .ok_or(BuildCredentialError(
            "Failed to enter the id into the credential".to_string(),
        ))?;

    // Add credential status
    let status_list_url = get_status_list_url(credential_status_index)?;

    credential_data.insert_if_none(
        &["credentialStatus"],
        json!({
            "type": StatusListTyp::Jwt.to_string(),
            "id": status_list_url.to_string(),
            "uri": status_list_url.to_string(),
            "idx": credential_status_index,
        }),
    );

    if let Some(expiration_date) = expires_at {
        credential_data
            .insert_at_path(&["expirationDate"], json!(expiration_date))
            .ok_or(BuildCredentialError(
                "Failed to enter the expirationDate into the credential".to_string(),
            ))?;
    }

    // Loop through all the items in the `type` array in reverse until we find a match.
    // This looping assumes the most specific type to match on is the latest one in the array.
    // This is an implicit consequence of the typing rules in digital credential formats.
    // For example, for OBv3 as well as ELM the first type is `VerifiableCredential` and the second type is its own type (e.g. `OpenBadgeCredential`/`EuropeanDigitalCredential`).
    for credential_type in credential_types.iter().rev() {
        match credential_type.as_str() {
            "VerifiableCredential" => {
                credential_data
                    .insert_if_none(&["name"], json!(credential_name))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the name into the credential".to_string(),
                    ))?;

                // JwtVcJson is still based on VC DM 1.1, while VcSdJwt (vc+sd-jwt) is based on VC DM 2.0.
                match credential_configuration.credential_format {
                    CredentialFormats::JwtVcJson(_) => {
                        credential_data
                            .insert_if_none(&["@context"], json!(["https://www.w3.org/2018/credentials/v1"]))
                            .ok_or(BuildCredentialError(
                                "Failed to enter the @context into the credential".to_string(),
                            ))?;
                    }
                    CredentialFormats::VcSdJwt(_) => {
                        credential_data
                            .insert_if_none(&["@context"], json!(["https://www.w3.org/ns/credentials/v2"]))
                            .ok_or(BuildCredentialError(
                                "Failed to enter the @context into the credential".to_string(),
                            ))?;
                    }
                    // TODO: this is actually a hard enforcement that a VC (DM 1.1 & 2) cannot be issued as dc+sd-jwt. Do we want that?
                    _ => {
                        return Err(UnsupportedCredentialFormat(serde_json::json!(
                            credential_configuration.credential_format
                        )));
                    }
                }

                return Ok(credential_data.clone());
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

                return Ok(credential_data.clone());
            }
            "EuropeanDigitalCredential" => {
                // Currently the ELM schema still references VC DM 1.1.
                // It seems like they will be moving to VC DM 2.0. but for now we need to be compatible with both.
                // TODO: remove once the ELM schema has been updated to VC DM 2.0.
                credential_data
                    .insert_if_none(&["@context"], json!(["https://www.w3.org/2018/credentials/v1"]))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the @context into the credential".to_string(),
                    ))?;

                if let Some(expiration_date) = expires_at {
                    credential_data
                        .insert_at_path(&["expirationDate"], json!(expiration_date))
                        .ok_or(BuildCredentialError(
                            "Failed to enter the expirationDate date into the credential".to_string(),
                        ))?;
                }
                // TODO: Due to the complexity of the different allowed issuer types (Agent, Person, Organisation),
                // We will keep it simple for now and only pass a placeholder eIDAS Legal Identifier.
                // As long as organisations don't have their eIDAS Legal Identifier there can be made no official `issuer` nor ELM anyway.
                credential_data.insert_if_none(&["issuer"], json!("urn:epass:org:1"));

                // No fields in credentialProfiles are actually required by the ELM schema
                // For now entering this dummy default as it is the same in every example under this link:
                // https://github.com/european-commission-empl/European-Learning-Model/tree/master/Credentials/JSON-LD%20Examples%20(ELM%20v3)
                credential_data
                    .insert_if_none(
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

                // TODO: this is currently hard coded, it can remain so until the use of this property (and all of ELM) becomes more clear and it has purpose to the user.
                // Also the `language` and `primaryLanguage` properties have no required fields but following the examples in the link above we use the current as sensible defaults.
                credential_data
                    .insert_if_none(
                        &["displayParameter"],
                        json!({
                            "id": "urn:epass:displayParameter:1",
                            "type": "DisplayParameter",
                            "title": {
                                "en": credential_name
                            },
                            "primaryLanguage": {
                                "id": "http://publications.europa.eu/resource/authority/language/ENG",
                                "type": "Concept",
                                "inScheme": {
                                "id": "http://publications.europa.eu/resource/authority/language",
                                "type": "ConceptScheme"
                                },
                                "notation": "language",
                                "prefLabel": {
                                "en": "English"
                                }
                            },
                            "language": {
                                "id": "http://publications.europa.eu/resource/authority/language/ENG",
                                "type": "Concept",
                                "inScheme": {
                                "id": "http://publications.europa.eu/resource/authority/language",
                                "type": "ConceptScheme"
                                },
                                "notation": "language",
                                "prefLabel": {
                                "en": "English"
                                }
                            },
                            "individualDisplay": {
                                "id": "urn:epass:individualDisplay:1",
                                "type": "IndividualDisplay",
                                "language": {
                                    "id": "http://publications.europa.eu/resource/authority/language/ENG",
                                    "type": "Concept",
                                    "inScheme": {
                                        "id": "http://publications.europa.eu/resource/authority/language",
                                        "type": "ConceptScheme"
                                    },
                                    "notation": "language",
                                    "prefLabel": {
                                        "en": "English"
                                    }
                                },
                                "displayDetail": {
                                    "id": "urn:epass:displayDetail:1",
                                    "type": "DisplayDetail",
                                    "page": 1,
                                    "image": {
                                        "id": "urn:epass:mediaObject:1",
                                        "type": "MediaObject",
                                        // TODO: this field needs an actual baked in image, binary data, with live data the encoding and type need to be changed accordingly
                                        "content": "[PLACEHOLDER]",
                                        "contentEncoding": {
                                            "id": "http://data.europa.eu/snb/encoding/6146cde7dd",
                                            "type": "Concept",
                                            "inScheme": {
                                                "id": "http://data.europa.eu/snb/encoding/25831c2",
                                                "type": "ConceptScheme"
                                            },
                                            "prefLabel": {
                                                "en": "base64"
                                            }
                                        },
                                        "contentType": {
                                            "id": "http://publications.europa.eu/resource/authority/file-type/JPEG",
                                            "type": "Concept",
                                            "inScheme": {
                                                "id": "http://publications.europa.eu/resource/authority/file-type",
                                                "type": "ConceptScheme"
                                            },
                                            "notation": "file-type",
                                            "prefLabel": {
                                                "en": "JPEG"
                                            }
                                        }
                                    },
                                },
                            },
                        }),
                    )
                    .ok_or(BuildCredentialError(
                        "Failed to enter the displayParameter into the credential".to_string(),
                    ))?;

                credential_data
                    .insert_if_none(
                        &["credentialSchema"],
                        json!({
                            "id": "https://eudiw.org/credentials/schemas/EuropeanDigitalCredentialV3_3.json",
                            "type": "JsonSchema"
                        }),
                    )
                    .ok_or(BuildCredentialError(
                        "Failed to enter the credentialSchema into the credential".to_string(),
                    ))?;

                // The ELM Data Model only allows two different types: "CredentialStatus", "TrustedCredentialStatus2021".
                // Therefore, we have no choice but to type it as the generic "CredentialStatus".
                credential_data
                    .insert_at_path(&["credentialStatus", "type"], json!("CredentialStatus"))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the credentialStatus.type into the credential".to_string(),
                    ))?;

                return Ok(credential_data.clone());
            }
            _ => continue,
        }
    }

    Err(BuildCredentialError(
        "None of the provided credential types are supported".to_string(),
    ))
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

        // Navigate/create path to parent of final key
        for key in parent_path {
            current_value = current_value
                // TODO: add array handling here too?
                .as_object_mut()?
                .entry((*key).to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        }

        // Insert the value at the final key if it doesn't exist
        current_value
            .as_object_mut()?
            .entry(last_key.to_string())
            .or_insert(value);

        Some(self)
    }
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
                proofs: None,
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
    use oid4vci::credential_issuer::credential_configurations_supported::CredentialMetadata;
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
        },
        credential_issuer::credential_configurations_supported::{
            AlgIdentifier, CredentialConfigurationsSupportedDisplay, Logo,
        },
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

    pub const OPENBADGE_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6Imh0dHBzOi8vZXhhbXBsZS5jb20vY3JlZGVudGlhbHMvMzUyNyIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy9ucy9jcmVkZW50aWFscy92MiIsImh0dHBzOi8vcHVybC5pbXNnbG9iYWwub3JnL3NwZWMvb2IvdjNwMC9jb250ZXh0LTMuMC4zLmpzb24iXSwiaWQiOiJodHRwczovL2V4YW1wbGUuY29tL2NyZWRlbnRpYWxzLzM1MjciLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIiwiT3BlbkJhZGdlQ3JlZGVudGlhbCJdLCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInZhbGlkRnJvbSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwibmFtZSI6IlRlYW13b3JrIEJhZGdlIiwiY3JlZGVudGlhbFN1YmplY3QiOnsiaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInR5cGUiOlsiQWNoaWV2ZW1lbnRTdWJqZWN0Il0sImFjaGlldmVtZW50Ijp7ImlkIjoiaHR0cHM6Ly9leGFtcGxlLmNvbS9hY2hpZXZlbWVudHMvMjFzdC1jZW50dXJ5LXNraWxscy90ZWFtd29yayIsInR5cGUiOiJBY2hpZXZlbWVudCIsImNyaXRlcmlhIjp7Im5hcnJhdGl2ZSI6IlRlYW0gbWVtYmVycyBhcmUgbm9taW5hdGVkIGZvciB0aGlzIGJhZGdlIGJ5IHRoZWlyIHBlZXJzIGFuZCByZWNvZ25pemVkIHVwb24gcmV2aWV3IGJ5IEV4YW1wbGUgQ29ycCBtYW5hZ2VtZW50LiJ9LCJkZXNjcmlwdGlvbiI6IlRoaXMgYmFkZ2UgcmVjb2duaXplcyB0aGUgZGV2ZWxvcG1lbnQgb2YgdGhlIGNhcGFjaXR5IHRvIGNvbGxhYm9yYXRlIHdpdGhpbiBhIGdyb3VwIGVudmlyb25tZW50LiIsIm5hbWUiOiJUZWFtd29yayJ9fSwiY3JlZGVudGlhbFN0YXR1cyI6eyJpZCI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsInR5cGUiOiJzdGF0dXNsaXN0K2p3dCIsInVyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsImlkeCI6MH19LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjowfX19.F-Iuig6Em7T_SxqO6h4cTHcdIHy0yKwlCn1m2653sPK3TlT7NAFvrWL-35wrjKxSKo4j1M6Y0M6E3yEUyEHyDw";

    pub const W3C_VC_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIl0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiXSwiY3JlZGVudGlhbFN1YmplY3QiOnsiaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4iLCJkZWdyZWUiOnsidHlwZSI6Ik1hc3RlckRlZ3JlZSIsIm5hbWUiOiJNYXN0ZXIgb2YgT2NlYW5vZ3JhcGh5In19LCJpc3N1ZXIiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsImlzc3VhbmNlRGF0ZSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwiY3JlZGVudGlhbFN0YXR1cyI6eyJpZCI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsInR5cGUiOiJzdGF0dXNsaXN0K2p3dCIsInVyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsImlkeCI6MH0sInZhbGlkRnJvbSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwibmFtZSI6IlZlcmlmaWFibGUgQ3JlZGVudGlhbCJ9LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjowfX19.GVmn2k8JjBqNS2MdEaO-GFN0Q6npmJgT3xGWEfCIxsKumJg6g8ZOxFms-_B7eh9qyf1smdZ22F9EjIt-4D4lDQ";

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
                    })
                    .into(),
                }),
                cryptographic_binding_methods_supported: vec!["did:key".to_string(), "did:jwk".to_string(),],
                credential_signing_alg_values_supported: vec![AlgIdentifier::String("EdDSA".to_string())],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec![AlgIdentifier::String("EdDSA".to_string())],
                    },
                )]),
                credential_metadata: Some(CredentialMetadata {
                    display: Some(vec![CredentialConfigurationsSupportedDisplay {
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
                    }]),
                    claims: None,
                }),
                ..Default::default()
            };
        pub static ref W3C_VC_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    parameters: (CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string()],
                    })
                    .into()
                }),
                cryptographic_binding_methods_supported: vec!["did:jwk".to_string(), "did:key".to_string(),],
                credential_signing_alg_values_supported: vec![
                    AlgIdentifier::String("ES256".to_string()),
                    AlgIdentifier::String("EdDSA".to_string())
                ],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec![
                            AlgIdentifier::String("ES256".to_string()),
                            AlgIdentifier::String("EdDSA".to_string())
                        ],
                    },
                )]),
                credential_metadata: Some(CredentialMetadata {
                    display: Some(vec![CredentialConfigurationsSupportedDisplay {
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
                    }]),
                    claims: None,
                }),
                ..Default::default()
            };
        pub static ref DC_SD_JWT_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::DcSdJwt(Parameters {
                    parameters: ("http://localhost:3033/vct/U0QtSldU/0".to_string()).into()
                }),
                cryptographic_binding_methods_supported: vec!["did:jwk".to_string(), "did:key".to_string(),],
                credential_signing_alg_values_supported: vec![
                    AlgIdentifier::String("ES256".to_string()),
                    AlgIdentifier::String("EdDSA".to_string())
                ],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec![
                            AlgIdentifier::String("ES256".to_string()),
                            AlgIdentifier::String("EdDSA".to_string())
                        ],
                    },
                )]),
                credential_metadata: Some(CredentialMetadata {
                    display: Some(vec![CredentialConfigurationsSupportedDisplay {
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
                    }]),
                    claims: None
                }),
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
                credential_signing_alg_values_supported: vec![
                    AlgIdentifier::String("ES256".to_string()),
                    AlgIdentifier::String("EdDSA".to_string())
                ],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec![
                            AlgIdentifier::String("ES256".to_string()),
                            AlgIdentifier::String("EdDSA".to_string())
                        ],
                    },
                )]),
                credential_metadata: Some(CredentialMetadata {
                    display: Some(vec![CredentialConfigurationsSupportedDisplay {
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
                    }]),
                    claims: None
                }),
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
            "https://www.w3.org/ns/credentials/v2",
            "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.3.json"
          ],
          "id": "https://example.com/credentials/3527",
          "type": ["VerifiableCredential", "OpenBadgeCredential"],
          "issuer": {
            "id": "https://my-domain.example.org/",
            "type": "Profile",
            "name": "UniCore"
          },
          "validFrom": "2010-01-01T00:00:00Z",
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
          },
          "validFrom": "2010-01-01T00:00:00Z",
          "name": "Verifiable Credential"
        });
        pub static ref UNSIGNED_DC_SD_JWT_CREDENTIAL: serde_json::Value = json!({
            "vct": "http://localhost:3033/vct/U0QtSldU/0",
            "first_name": "Ferris",
            "last_name": "Rustacean"
        });
        // TODO: should the `vct` claim (and others) already be added here? I would say all building/compiling of data should be separate from signing.
        pub static ref UNSIGNED_VC_SD_JWT_CREDENTIAL: serde_json::Value = json!({
          "@context": [ "https://www.w3.org/ns/credentials/v2" ],
        //   "type": [ "VerifiableCredential" ], should this be in or out?
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
          },
          "name": "VCDM2.0 SD-JWT Credential"
        });
    }
}
