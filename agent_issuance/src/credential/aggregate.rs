use super::entity::Data;
use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self, *};
use crate::credential::event::CredentialEvent;
use crate::credential::openapi::{credential_configurations_supported, holder_notifications, status_type};
use crate::services::IssuanceServices;
use agent_library::json_schema_validation::{CredentialType, JsonSchemaError};
use agent_shared::config::{config, get_preferred_did_method, get_preferred_signing_algorithm, AlgorithmExt};
use agent_shared::serde_json_value_ext::SerdeJsonValueExt;
use chrono::{DateTime, Utc};
use cqrs_es::{event_sink::EventSink, Aggregate};
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
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, utoipa::ToSchema)]
pub enum Status {
    #[default]
    Pending,
    Issued,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, utoipa::ToSchema)]
pub struct CredentialStatus {
    pub index: usize,
    #[schema(schema_with = status_type)]
    pub status: StatusType,
    pub status_list_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, utoipa::ToSchema)]
pub struct Credential {
    #[serde(rename = "id")]
    pub credential_id: String,
    pub notification_id: Option<String>,
    pub data: Option<Data>,
    #[schema(schema_with = credential_configurations_supported)]
    pub credential_configuration: CredentialConfigurationsSupportedObject,
    pub signed: Option<serde_json::Value>,
    #[schema(inline)]
    pub status: Status,
    #[schema(schema_with = holder_notifications)]
    pub holder_notifications: Vec<NotificationRequest>,
    pub credential_status: CredentialStatus,
    #[schema(value_type = Option<String>)]
    pub created_at: Option<DateTime<Utc>>,
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<DateTime<Utc>>,
}

impl Aggregate for Credential {
    type Command = CredentialCommand;
    type Event = CredentialEvent;
    type Error = CredentialError;
    type Services = Arc<IssuanceServices>;

    const TYPE: &'static str = "credential";

    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use CredentialCommand::*;
        use CredentialError::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
            CreateUnsignedCredential {
                credential_id,
                data,
                credential_configuration,
                expires_at,
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

                let mut credential_data = data.raw.clone();

                let credential_data = match &credential_configuration.credential_format {
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

                        build_unsigned_w3c_credential_data(
                            type_,
                            &mut credential_data,
                            &credential_configuration,
                            &credential_id,
                            expires_at,
                        )?
                    }
                    CredentialFormats::DcSdJwt(Parameters::<DcSdJwt> {
                        parameters: DcSdJwtParameters { vct },
                    }) => {
                        credential_data.insert_at_path(&["vct"], json!(vct));

                        credential_data
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

                        build_unsigned_w3c_credential_data(
                            type_,
                            &mut credential_data,
                            &credential_configuration,
                            &credential_id,
                            expires_at,
                        )?
                    }
                    _ => {
                        return Err(UnsupportedCredentialFormat(serde_json::json!(
                            credential_configuration.credential_format
                        )))
                    }
                };

                Ok(vec![UnsignedCredentialCreated {
                    credential_id,
                    notification_id: Some(notification_id),
                    data: Data { raw: credential_data },
                    credential_configuration,
                    created_at: Some(created_at),
                    expires_at,
                }])
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
                status_list_id,
                index,
            } => {
                if self.signed.is_some() && !overwrite {
                    return Ok(());
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

                // Status claim as per the IETF OAuth Token Status List specification.
                let status_list_url = get_status_list_url(status_list_id)?;

                let status_claim = sd_jwt_vc::Status(StatusMechanism::StatusList(StatusListRef {
                    idx: index,
                    uri: status_list_url.clone(),
                }));

                // The sensible default for the jti is equal to the credential root `id` field
                let jti: Option<Url> = self
                    .data
                    .as_ref()
                    .and_then(|data| data.raw.get("id"))
                    .and_then(|id| id.as_str())
                    .and_then(|id| Url::parse(id).ok());

                let credential_data = self.data.as_ref().ok_or(InvalidCredentialDataError)?.raw.clone();

                // TODO: can this holder binding also be used in JwtVcJson?
                // If proof is provided then set the holder_kid needs to be extracted to set the `cnf` claim.
                let holder_kid = proofs.and_then(|proofs| {
                    // TODO: Support batch credential issuance. See https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-batch-credential-issuance
                    jsonwebtoken::decode_header(proofs.jwt.first()?)
                        .ok()
                        .and_then(|header| header.kid)
                });

                // Set the jwt claims through the specific builder and build the JWT which will be signed at the bottom of each match arm
                let signed_credential = match &self.credential_configuration.credential_format {
                    CredentialFormats::JwtVcJson(_) => {
                        let credential_data = build_signed_w3c_credential_data(
                            credential_data,
                            created_at,
                            iss.to_string(),
                            subject_id.clone(),
                            index,
                            status_list_url.to_string(),
                        )?;

                        // TODO: Would it be more straightforward to add the JWT claims similarly to all our jwt formats?

                        let mut vc_jwt_builder = VerifiableCredentialJwt::builder()
                            .iss(iss.to_string())
                            .iat(iat.to_unix())
                            .nbf(iat.to_unix()); // setting the `nbf` to `iat` makes the JWT immediately usable

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
                        let credential_data = build_signed_w3c_credential_data(
                            credential_data,
                            created_at,
                            iss.to_string(),
                            subject_id.clone(),
                            index,
                            status_list_url.to_string(),
                        )?;

                        // Get the kid for the header of the VcSdJwt
                        let issuer = &services.issuer;
                        let algorithm = get_preferred_signing_algorithm();
                        let kid = issuer
                            .key_id(&get_preferred_did_method().to_string(), algorithm)
                            .await
                            .ok_or(KeyIdError)?;

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

                        builder = builder
                            .insert_claim("iss", iss)
                            .map_err(|_| BuildCredentialError("Failed to insert 'iss' claim".to_string()))?;

                        // By default set all custom claims to concealable.
                        // TODO: This only makes the credentialSubject properties concealable. Further research needed to see which properties should be conceleable, also depending on what Credential Data Model.
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
                    credential_status: CredentialStatus {
                        index,
                        status: StatusType::VALID,
                        status_list_url: status_list_url.to_string(),
                    },
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
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
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
                created_at,
                expires_at,
            } => {
                self.credential_id = credential_id;
                self.data.replace(data);
                self.credential_configuration = *credential_configuration;
                self.notification_id = notification_id;
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
                credential_status,
                status,
            } => {
                self.credential_id = credential_id;
                self.signed.replace(signed_credential);
                self.credential_status = credential_status;
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

/// This function sets the following values
/// - issuer.id
/// - credentialSubject.id
/// - validFrom/issuanceDate/issued
fn build_signed_w3c_credential_data(
    mut credential_data: serde_json::Value,
    created_at: String,
    iss: String,
    subject_id: Option<String>,
    status_index: usize,
    status_list_url: String,
) -> Result<serde_json::Value, CredentialError> {
    let credential_types = credential_data
        .get("type")
        .and_then(|t| t.as_array())
        .ok_or(InvalidCredentialDataError)?
        .iter()
        .filter_map(|t| t.as_str())
        .map(|s| s.to_string())
        .collect::<Vec<String>>();

    // Add both issuanceDate and validFrom for forward/backward (maximum) compatibility.
    credential_data
        .insert_if_none(&["issuanceDate"], json!(created_at))
        .ok_or(BuildCredentialError(
            "Failed to enter the issuanceDate date into the credential".to_string(),
        ))?;
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

    // Add credential status
    credential_data.insert_if_none(
        &["credentialStatus"],
        json!({
            "type": StatusListTyp::Jwt.to_string(),
            "id": status_list_url.to_string(),
            "uri": status_list_url.to_string(),
            "idx": status_index,
        }),
    );

    // Loop through all the items in the `type` array in reverse until we find a match.
    // This looping assumes the most specific type to match on is the latest one in the array.
    // This is an implicit consequence of the typing rules in digital credential formats.
    // For example, for OBv3 as well as ELM the first type is `VerifiableCredential` and the second type is its own type (e.g. `OpenBadgeCredential`/`EuropeanDigitalCredential`).
    for credential_type in credential_types.iter().rev() {
        match credential_type.as_str() {
            "VerifiableCredential" => {
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
                // The following link explains the difference between `issued`, `issuanceDate` and `validFrom`:
                // https://europa.eu/europass/elm-browser/homepage/3-2-0/edc-generic-no-cv_en.html
                credential_data
                    .insert_if_none(&["issued"], json!(created_at))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the issued date into the credential".to_string(),
                    ))?;

                // The ELM Data Model only allows two different types: "CredentialStatus", "TrustedCredentialStatus2021".
                // Therefore, we have no choice but to type it as the generic "CredentialStatus".
                credential_data
                    .insert_at_path(&["credentialStatus", "type"], json!("CredentialStatus"))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the credentialStatus.type into the credential".to_string(),
                    ))?;

                // TODO: Due to the complexity of the different allowed issuer types (Agent, Person, Organisation) we keep it simple for now and pass the issuer as entered at the top of this fn.
                // As long as organisations don't have their eIDAS Legal Identifier there can be made no official `issuer` nor ELM anyway.

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
/// NOTE: Keep in mind that all data used during signing (SignCredential) for the JWT claims is also only then set for its Credential Data Model counterparts, this includes:
/// - `issuer.id` (iss)
/// - `credentialSubject.id` (sub)
/// - `issuanceDate`/`validFrom`/`issued` (iat)
///
/// This function sets the following fields if applicable for the specific Credential Data Model:
/// - `@context`, if not already set
/// - `name`, if not already set
/// - `id`, set to the aggregate credential_id as a URN. This means it's non-configurable by API users. Main reason for this is our reliance on the credential id to be equal to the aggregate id for the public link flow.
/// - `issuer.name`, reflecting the UniCore configuration
/// - `credentialStatus`, if not already set, according to the IETF OAuth Token Status List specification in combination with the DIIP profile.
/// - `expirationDate`, if expires_at is provided
/// - All ELM required fields:
fn build_unsigned_w3c_credential_data(
    credential_types: &[String],
    credential_data: &mut serde_json::Value,
    credential_configuration: &CredentialConfigurationsSupportedObject,
    credential_id: &str,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<serde_json::Value, CredentialError> {
    // Set to the aggregate credential_id as a URN, because this needs to be a valid URL by spec. This means it's non-configurable by API users. Main reason for this is our reliance on the credential id to be equal to the aggregate id for the public link flow.

    #[allow(unused_variables)]
    // Allow because this simply is a bug on how the test_utils is interpreted by the compiler.
    let root_id = uuid::Uuid::parse_str(credential_id)
        .map_err(|e| BuildCredentialError(format!("Failed to parse credential_id as UUID: {}", e)))?;
    #[cfg(feature = "test_utils")]
    let root_id = uuid::Uuid::parse_str(test_utils::CREDENTIAL_ID).expect("Static test UUID should always parse");

    credential_data
        .insert_at_path(&["id"], json!(root_id.urn()))
        .ok_or(BuildCredentialError(
            "Failed to enter the id into the credential".to_string(),
        ))?;

    let credential_name = credential_configuration
        .credential_metadata
        .as_ref()
        .and_then(|meta| meta.display.as_ref())
        .and_then(|display| display.first())
        .map(|d| d.name.clone());

    // This defaults the name to the credential configuration name if no name is provided.
    if let Some(credential_name) = &credential_name {
        credential_data.insert_if_none(&["name"], json!(credential_name));
    }

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

    // Set both the expirationDate and validUntil for forward/backward (maximum) compatibility.
    if let Some(expiration_date) = expires_at {
        credential_data
            .insert_at_path(&["validUntil"], json!(expiration_date))
            .ok_or(BuildCredentialError(
                "Failed to enter the validUntil date into the credential".to_string(),
            ))?;
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
                // Our default is VC DM 2.0, but JwtVcJson is still based on VC DM 1.1.
                if matches!(
                    credential_configuration.credential_format,
                    CredentialFormats::JwtVcJson(_)
                ) {
                    credential_data
                        .insert_if_none(&["@context"], json!(["https://www.w3.org/2018/credentials/v1"]))
                        .ok_or(BuildCredentialError(
                            "Failed to enter the @context into the credential".to_string(),
                        ))?;
                } else {
                    credential_data
                        .insert_if_none(&["@context"], json!(["https://www.w3.org/ns/credentials/v2"]))
                        .ok_or(BuildCredentialError(
                            "Failed to enter the @context into the credential".to_string(),
                        ))?;
                }

                // Validate credential data before returning
                let validation_result = CredentialType::VerifiableCredential.validate(credential_data);

                match validation_result {
                    Ok(_) => return Ok(credential_data.clone()),
                    Err(mut errors) => {
                        if filter_schema_errors(&mut errors) {
                            return Ok(credential_data.clone());
                        }
                        return Err(InvalidCredentialPayloadError(errors));
                    }
                }
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

                // Set the name to "OpenBadge Credential" if no name is provided, as it is required by the OBv3 schema.
                credential_data
                    .insert_if_none(&["name"], json!("OpenBadge Credential"))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the name into the credential".to_string(),
                    ))?;

                // Validate credential data before returning
                let validation_result = CredentialType::OpenBadgeCredential.validate(credential_data);

                match validation_result {
                    Ok(_) => return Ok(credential_data.clone()),
                    Err(mut errors) => {
                        if filter_schema_errors(&mut errors) {
                            return Ok(credential_data.clone());
                        }
                        return Err(InvalidCredentialPayloadError(errors));
                    }
                }
            }
            "EuropeanDigitalCredential" => {
                // Currently the ELM schema still references VC DM 1.1, although it seems likely they will be moving to VC DM 2.0.
                // For now we need to add both contexts (which is a conflict) to be able to issue the ELM as a valid SD-JWT.
                // TODO: remove once the ELM schema has been updated to VC DM 2.0.
                credential_data
                    .insert_if_none(
                        &["@context"],
                        json!([
                            "https://www.w3.org/2018/credentials/v1",
                            "https://www.w3.org/ns/credentials/v2"
                        ]),
                    )
                    .ok_or(BuildCredentialError(
                        "Failed to enter the @context into the credential".to_string(),
                    ))?;

                // No fields in credentialProfiles are actually required by the ELM schema.
                // Since we do not fully understand the use of this property yet, we enter an empty object.
                credential_data
                    .insert_if_none(&["credentialProfiles"], json!({}))
                    .ok_or(BuildCredentialError(
                        "Failed to enter the credentialProfiles into the credential".to_string(),
                    ))?;

                // TODO: this is currently hard coded, it can remain so until the use of this property (and all of ELM) becomes more clear and it has purpose to the user.
                // Also the `language` and `primaryLanguage` properties have no required fields but following the examples in the link above we use the current as sensible defaults.
                credential_data
                    .insert_if_none(
                        &["displayParameter"],
                        json!({
                            "title": {
                                "en": credential_name
                            },
                            "language": {},
                            "primaryLanguage": {},
                            "individualDisplay": {
                                "language": {},
                                "displayDetail": {
                                    "page": 1,
                                    "image": {
                                        // TODO: this field needs an actual baked in image, binary data, with live data the encoding and type need to be changed accordingly
                                        "content": "[PLACEHOLDER]",
                                        "contentEncoding": {},
                                        "contentType": {}
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

                // Validate credential data before returning
                let validation_result = CredentialType::EuropeanDigitalCredential.validate(credential_data);

                match validation_result {
                    Ok(_) => return Ok(credential_data.clone()),
                    Err(mut errors) => {
                        if filter_schema_errors(&mut errors) {
                            return Ok(credential_data.clone());
                        }
                        return Err(InvalidCredentialPayloadError(errors));
                    }
                }
            }
            _ => continue,
        }
    }

    Err(BuildCredentialError(
        "None of the provided credential types are supported".to_string(),
    ))
}

/// Helper to filter schema errors for fields which are set during signing (SignCredential),
/// and therefore not present during the validation in the CreateUnsignedCredential step.
fn filter_schema_errors(errors: &mut JsonSchemaError) -> bool {
    if let JsonSchemaError::CredentialValidationError(_, ref mut errs) = errors {
        errs.retain(|error| {
            !(error.contains("issuer/id")
                || error.contains("credentialSubject/id")
                || error.contains("issuanceDate")
                || error.contains("validFrom")
                || error.contains("issued")
                || (error.contains("issuer") && error.contains("id")))
        });
        errs.is_empty()
    } else {
        false
    }
}

pub fn get_status_list_url(id: String) -> Result<identity_core::common::Url, CredentialError> {
    let mut status_list_url = config().ietf_oauth_token_status_list_uri.clone();
    status_list_url
        .path_segments_mut()
        .map_err(|_| CredentialError::InvalidCredentialStatus)?
        .push(&id);

    Ok(status_list_url.into())
}

#[cfg(test)]
pub mod credential_tests {
    use super::test_utils::*;
    use super::*;

    use agent_shared::config::TESTINDEX;
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
    #[case::jwt_vc_json_vc1_1(
        BASIC_CREDENTIAL_SUBJECT.clone(),
        JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_VC1_1_CREDENTIAL.clone()
    )]
    #[case::jwt_vc_json_obv3(
        OPENBADGE_CREDENTIAL_SUBJECT.clone(),
        JWT_VC_JSON_OBv3_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_OPENBADGE_CREDENTIAL.clone()
    )]
    #[case::jwt_vc_json_elm(
        BASIC_CREDENTIAL_SUBJECT.clone(),
        JWT_VC_JSON_ELM_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_ELM_CREDENTIAL.clone()
    )]
    #[case::dc_sd_jwt(
        // DC SD-JWT is a flat structure, so no nested properties. Therefore we flatten the credentialSubject here by retrieving its nested keys only.
        BASIC_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
        DC_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_DC_SD_JWT_CREDENTIAL.clone()
    )]
    #[case::vc2_sd_jwt(
        BASIC_CREDENTIAL_SUBJECT.clone(),
        VC2_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_VC2_SD_JWT_CREDENTIAL.clone()
    )]
    #[case::obv3_sd_jwt(
        OPENBADGE_CREDENTIAL_SUBJECT.clone(),
        OBv3_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_OPENBADGE_CREDENTIAL.clone()
    )]
    #[case::elm_sd_jwt(
        BASIC_CREDENTIAL_SUBJECT.clone(),
        ELM_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_ELM_CREDENTIAL.clone()
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
            })
            .then_expect_events(vec![CredentialEvent::UnsignedCredentialCreated {
                credential_id,
                data: Data {
                    raw: unsigned_credential,
                },
                notification_id: Some(notification_id.clone()),
                credential_configuration: Box::new(credential_configuration),
                created_at: Some(created_at),
                expires_at: None,
            }])
    }

    // TODO: enable sd-jwt testing, since the salts change everytime we need to come up with an alternative to `assert_eq!`,
    // which is used by the `.then_expect_events` method in the tests.
    //
    // #[case::dc_sd_jwt(
    //     UNSIGNED_DC_SD_JWT_CREDENTIAL.clone(),
    //     DC_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
    //     DC_SD_JWT.to_string()
    // )]
    // #[case::vc2_sd_jwt(
    //     UNSIGNED_VC2_SD_JWT_CREDENTIAL.clone(),
    //     VC2_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
    //     VC2_SD_JWT.to_string()
    // )]
    // #[case::obv3_sd_jwt(
    //     UNSIGNED_OPENBADGE_CREDENTIAL.clone(),
    //     OBv3_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
    //     OBV3_SD_JWT.to_string()
    // )]
    // #[case::elm_sd_jwt(
    //     UNSIGNED_ELM_CREDENTIAL.clone(),
    //     ELM_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
    //     ELM_SD_JWT.to_string()
    // )]
    #[rstest]
    #[case::jwt_vc_json_vc1_1(
        UNSIGNED_VC1_1_CREDENTIAL.clone(),
        JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION.clone(),
        JWT_VC_JSON_VC1_1_JWT.to_string(),
    )]
    #[case::jwt_vc_json_obv3(
        UNSIGNED_OPENBADGE_CREDENTIAL.clone(),
        JWT_VC_JSON_OBv3_CREDENTIAL_CONFIGURATION.clone(),
        JWT_VC_JSON_OBV3_JWT.to_string(),
    )]
    #[case::jwt_vc_json_elm(
        UNSIGNED_ELM_CREDENTIAL.clone(),
        JWT_VC_JSON_ELM_CREDENTIAL_CONFIGURATION.clone(),
        JWT_VC_JSON_ELM_JWT.to_string()
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
        use agent_shared::config::TEST_STATUS_LIST_ID;

        let credential_status = CredentialStatus {
            index: TESTINDEX,
            status_list_url: get_status_list_url(TEST_STATUS_LIST_ID.to_string())
                .unwrap()
                .to_string(),
            status: StatusType::VALID,
        };

        CredentialTestFramework::with(IssuanceServices::default().await)
            .given(vec![CredentialEvent::UnsignedCredentialCreated {
                credential_id: credential_id.clone(),
                data: Data {
                    raw: unsigned_credential,
                },
                credential_configuration: Box::new(credential_configuration),
                notification_id: None,
                created_at: Some(created_at),
                expires_at: None,
            }])
            .when(CredentialCommand::SignCredential {
                credential_id: credential_id.clone(),
                subject_id: Some(holder.identifier("did:key", Algorithm::EdDSA).await.unwrap()),
                overwrite: false,
                proofs: None,
                status_list_id: TEST_STATUS_LIST_ID.to_string(),
                index: TESTINDEX,
            })
            .then_expect_events(vec![CredentialEvent::CredentialSigned {
                credential_id,
                signed_credential: json!(verifiable_credential_jwt),
                credential_status,
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

    pub const CREDENTIAL_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    #[fixture]
    pub fn notification_id() -> String {
        "notification_id".to_string()
    }

    #[fixture]
    pub fn created_at() -> chrono::DateTime<chrono::Utc> {
        "2010-01-01T00:00:00Z".parse().unwrap()
    }

    // Test ID must still be parsable to a valid urn UUID.
    #[fixture]
    pub fn credential_id() -> String {
        CREDENTIAL_ID.to_string()
    }

    pub const JWT_VC_JSON_VC1_1_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6InVybjp1dWlkOjEyM2U0NTY3LWU4OWItMTJkMy1hNDU2LTQyNjYxNDE3NDAwMCIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIl0sImlkIjoidXJuOnV1aWQ6MTIzZTQ1NjctZTg5Yi0xMmQzLWE0NTYtNDI2NjE0MTc0MDAwIiwidHlwZSI6WyJWZXJpZmlhYmxlQ3JlZGVudGlhbCJdLCJjcmVkZW50aWFsU3ViamVjdCI6eyJmaXJzdF9uYW1lIjoiRmVycmlzIiwibGFzdF9uYW1lIjoiUnVzdGFjZWFuIiwiaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9LCJpc3N1ZXIiOnsibmFtZSI6IlVuaUNvcmUiLCJpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0In0sIm5hbWUiOiJWZXJpZmlhYmxlIENyZWRlbnRpYWwiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsInZhbGlkRnJvbSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwiY3JlZGVudGlhbFN0YXR1cyI6eyJ0eXBlIjoic3RhdHVzbGlzdCtqd3QiLCJpZCI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsInVyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsImlkeCI6MTIzfX0sInN0YXR1cyI6eyJzdGF0dXNfbGlzdCI6eyJ1cmkiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJpZHgiOjEyM319fQ.RLSGdmQ8PBZQzXIcwrjZafmBmo2gDf0UEIUfARSY6J1xkIcIjCVnEZU7Xs2HQUIZz_-_4VO0UZFRvDClN5ZJAQ";
    pub const JWT_VC_JSON_OBV3_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6InVybjp1dWlkOjEyM2U0NTY3LWU4OWItMTJkMy1hNDU2LTQyNjYxNDE3NDAwMCIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy9ucy9jcmVkZW50aWFscy92MiIsImh0dHBzOi8vcHVybC5pbXNnbG9iYWwub3JnL3NwZWMvb2IvdjNwMC9jb250ZXh0LTMuMC4zLmpzb24iXSwiaWQiOiJ1cm46dXVpZDoxMjNlNDU2Ny1lODliLTEyZDMtYTQ1Ni00MjY2MTQxNzQwMDAiLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIiwiT3BlbkJhZGdlQ3JlZGVudGlhbCJdLCJpc3N1ZXIiOnsidHlwZSI6IlByb2ZpbGUiLCJuYW1lIjoiVW5pQ29yZSIsImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QifSwibmFtZSI6IlRlYW13b3JrIEJhZGdlIiwiY3JlZGVudGlhbFN1YmplY3QiOnsidHlwZSI6WyJBY2hpZXZlbWVudFN1YmplY3QiXSwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbGUuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEiOnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY29nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNvZ25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbnQuIiwibmFtZSI6IlRlYW13b3JrIn0sImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QifSwiaXNzdWFuY2VEYXRlIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJ2YWxpZEZyb20iOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsImNyZWRlbnRpYWxTdGF0dXMiOnsidHlwZSI6InN0YXR1c2xpc3Qrand0IiwiaWQiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJ1cmkiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJpZHgiOjEyM319LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjoxMjN9fX0.oMaZSWsVZGcRyOZ-LalOuyUiQGmoo-Ur6dUfjOAMbAofW5ZISCyJaeLliQsacyxHEuEcxnD7v_QKrCRbeYL0DA";
    pub const JWT_VC_JSON_ELM_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6InVybjp1dWlkOjEyM2U0NTY3LWU4OWItMTJkMy1hNDU2LTQyNjYxNDE3NDAwMCIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly93d3cudzMub3JnL25zL2NyZWRlbnRpYWxzL3YyIl0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJFdXJvcGVhbkRpZ2l0YWxDcmVkZW50aWFsIl0sImlkIjoidXJuOnV1aWQ6MTIzZTQ1NjctZTg5Yi0xMmQzLWE0NTYtNDI2NjE0MTc0MDAwIiwiY3JlZGVudGlhbFN1YmplY3QiOnsiZmlyc3RfbmFtZSI6IkZlcnJpcyIsImxhc3RfbmFtZSI6IlJ1c3RhY2VhbiIsImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QifSwiaXNzdWVyIjp7Im5hbWUiOiJVbmlDb3JlIiwiaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9LCJuYW1lIjoiRXVyb3BlYW4gRGlnaXRhbCBDcmVkZW50aWFsIiwiY3JlZGVudGlhbFByb2ZpbGVzIjp7fSwiZGlzcGxheVBhcmFtZXRlciI6eyJ0aXRsZSI6eyJlbiI6IkV1cm9wZWFuIERpZ2l0YWwgQ3JlZGVudGlhbCJ9LCJwcmltYXJ5TGFuZ3VhZ2UiOnt9LCJsYW5ndWFnZSI6e30sImluZGl2aWR1YWxEaXNwbGF5Ijp7Imxhbmd1YWdlIjp7fSwiZGlzcGxheURldGFpbCI6eyJwYWdlIjoxLCJpbWFnZSI6eyJjb250ZW50IjoiW1BMQUNFSE9MREVSXSIsImNvbnRlbnRFbmNvZGluZyI6e30sImNvbnRlbnRUeXBlIjp7fX19fX0sImNyZWRlbnRpYWxTY2hlbWEiOnsiaWQiOiJodHRwczovL2V1ZGl3Lm9yZy9jcmVkZW50aWFscy9zY2hlbWFzL0V1cm9wZWFuRGlnaXRhbENyZWRlbnRpYWxWM18zLmpzb24iLCJ0eXBlIjoiSnNvblNjaGVtYSJ9LCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsInZhbGlkRnJvbSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwiY3JlZGVudGlhbFN0YXR1cyI6eyJ0eXBlIjoiQ3JlZGVudGlhbFN0YXR1cyIsImlkIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjoxMjN9LCJpc3N1ZWQiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiJ9LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjoxMjN9fX0.vc3rJjPdCIDGuLfBN0ZCXqLUl45LIjS1MC1HpGHa4ohjGxc2GPe5pMBLKWpksc_C-xf199vGFuWKYDZPncl2CQ";

    // TODO: enable sd-jwt testing, since the salts change everytime we need to come up with an alternative to `assert_eq!`
    //
    // pub const DC_SD_JWT: &str = "placeholder";
    // pub const VC2_SD_JWT: &str = "placeholder";
    // pub const OBV3_SD_JWT: &str = "placeholder";
    // pub const ELM_SD_JWT: &str = "placeholder";

    lazy_static! {
        pub static ref JWT_VC_JSON_OBv3_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
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
        pub static ref JWT_VC_JSON_ELM_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    parameters: (CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string(), "EuropeanDigitalCredential".to_string()],
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
                        name: "European Digital Credential".to_string(),
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
        pub static ref JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
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
        pub static ref VC2_SD_JWT_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
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
        pub static ref ELM_SD_JWT_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::VcSdJwt(Parameters {
                    parameters: (vc_sd_jwt::CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string(), "EuropeanDigitalCredential".to_string()],
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
                        name: "European Digital Credential".to_string(),
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
        pub static ref OBv3_SD_JWT_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::VcSdJwt(Parameters {
                    parameters: (vc_sd_jwt::CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
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
                        name: "Teamwork Badge".to_string(),
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

        // This is used for the DC SD-JWT, VC 1.1 and 2.0 test cases.
        pub static ref BASIC_CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "credentialSubject": {
                    "first_name": "Ferris",
                    "last_name": "Rustacean"
                }
            }
        );
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
        pub static ref UNSIGNED_OPENBADGE_CREDENTIAL: serde_json::Value = json!({
          "@context": [
            "https://www.w3.org/ns/credentials/v2",
            "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.3.json"
          ],
                    "id": format!("urn:uuid:{CREDENTIAL_ID}"),
          "type": ["VerifiableCredential", "OpenBadgeCredential"],
          "issuer": {
            "type": "Profile",
            "name": "UniCore"
          },
          "name": "Teamwork Badge",
          "credentialSubject": OPENBADGE_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
        });
        pub static ref UNSIGNED_VC1_1_CREDENTIAL: serde_json::Value = json!({
          "@context": [ "https://www.w3.org/2018/credentials/v1" ],
                    "id": format!("urn:uuid:{CREDENTIAL_ID}"),
          "type": [ "VerifiableCredential" ],
          "credentialSubject": BASIC_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
          "issuer": {
            "name": "UniCore"
          },
          "name": "Verifiable Credential"
        });
        pub static ref UNSIGNED_DC_SD_JWT_CREDENTIAL: serde_json::Value = json!({
            "vct": "http://localhost:3033/vct/U0QtSldU/0",
            "first_name": "Ferris",
            "last_name": "Rustacean"
        });
        pub static ref UNSIGNED_VC2_SD_JWT_CREDENTIAL: serde_json::Value = json!({
          "@context": [ "https://www.w3.org/ns/credentials/v2" ],
                    "id": format!("urn:uuid:{CREDENTIAL_ID}"),
          "type": [ "VerifiableCredential" ],
          "credentialSubject": BASIC_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
          "issuer": {
            "name": "UniCore"
          },
          "name": "VCDM2.0 SD-JWT Credential"
        });
        pub static ref UNSIGNED_ELM_CREDENTIAL: serde_json::Value = json!({
            "@context": [
                "https://www.w3.org/2018/credentials/v1",
                "https://www.w3.org/ns/credentials/v2"
            ],
            "type": [
                "VerifiableCredential",
                "EuropeanDigitalCredential"
            ],
            "id": format!("urn:uuid:{CREDENTIAL_ID}"),
            "credentialSubject": {
                "first_name": "Ferris",
                "last_name": "Rustacean"
            },
            "issuer": {
                "name": "UniCore"
            },
            "name": "European Digital Credential",
            "credentialProfiles": {},
            "displayParameter": {
                "title": {
                    "en": "European Digital Credential"
                },
                "primaryLanguage": {},
                "language": {},
                "individualDisplay": {
                    "language": {},
                    "displayDetail": {
                        "page": 1,
                        "image": {
                            "content": "[PLACEHOLDER]",
                            "contentEncoding": {},
                            "contentType": {}
                        }
                    }
                }
            },
            "credentialSchema": {
                "id": "https://eudiw.org/credentials/schemas/EuropeanDigitalCredentialV3_3.json",
                "type": "JsonSchema"
            }
        });
    }
}
