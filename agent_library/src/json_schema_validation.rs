// This folder is contains the lazy static ref Validators compiled from the JSON Schemas as to ensure easy compilation into the executable binary without the hassle of carrying over the JSON Schema files.
// Furthermore, it contains the items and functions needed for JSON Schema validation of credentials.
use jsonschema::{Retrieve, Uri, Validator};
use lazy_static::lazy_static;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use tracing::{info, warn};

lazy_static! {
    static ref SCHEMA_REGISTRY: HashMap<&'static str, Value> = {
        let mut json_schemas = HashMap::new();
            json_schemas.insert("VerifiableCredentialV1_1.json",
                serde_json::from_str(include_str!("json_schemas/VerifiableCredentialV1_1.json")).unwrap());
            json_schemas.insert("VerifiableCredentialV2.json",
                serde_json::from_str(include_str!("json_schemas/VerifiableCredentialV2.json")).unwrap());
            json_schemas.insert("OpenBadgeCredentialV3.json",
                serde_json::from_str(include_str!("json_schemas/OpenBadgeCredentialV3.json")).unwrap());
            json_schemas.insert("EDC_VerifiableCredentialV1_1.json",
                serde_json::from_str(include_str!("json_schemas/EDC_VerifiableCredentialV1_1.json")).unwrap());
            json_schemas.insert("EuropeanDigitalCredentialV3_3.json",
                serde_json::from_str(include_str!("json_schemas/EuropeanDigitalCredentialV3_3.json")).unwrap());
            json_schemas
        };

    // We can also compile the validators on the demand which would absolve the need for this static ref,
    // but since it is static data anyway we can reduce process time everytime validation is needed by only compiling it once.
    static ref VALIDATOR_REGISTRY: HashMap<&'static str, Validator> = {
        let mut validators = HashMap::new();
        validators.insert("VerifiableCredentialV1_1.json",
            compile_validator("VerifiableCredentialV1_1.json").expect("Failed to compile VerifiableCredentialV1_1"));
        validators.insert("VerifiableCredentialV2.json",
            compile_validator("VerifiableCredentialV2.json").expect("Failed to compile VerifiableCredentialV2"));
        validators.insert("EuropeanDigitalCredentialV3_3.json",
            compile_validator("EuropeanDigitalCredentialV3_3.json").expect("Failed to compile EuropeanDigitalCredentialV3_3"));
        validators.insert("OpenBadgeCredentialV3.json",
            compile_validator("OpenBadgeCredentialV3.json").expect("Failed to compile OpenBadgeCredentialV3"));
        validators
    };
}

#[derive(serde::Deserialize, PartialEq, Debug, strum::Display)]
pub enum CredentialType {
    VerifiableCredential,
    #[serde(alias = "AchievementCredential")]
    OpenBadgeCredential,
    EuropeanDigitalCredential,
    #[serde(other)]
    Unknown,
}

#[derive(serde::Deserialize, PartialEq, Debug, strum::Display)]
pub enum CredentialTypeVersion {
    VerifiableCredentialV1_1,
    VerifiableCredentialV2,
    EuropeanDigitalCredentialV3_3,
    OpenBadgeCredentialV3,
    #[serde(other)]
    Unknown,
}

impl CredentialTypeVersion {
    pub fn get_validator(&self) -> Result<&'static Validator, JsonSchemaError> {
        match self {
            CredentialTypeVersion::VerifiableCredentialV1_1 => VALIDATOR_REGISTRY
                .get("VerifiableCredentialV1_1.json")
                .ok_or(JsonSchemaError::GetCredentialTypeError(
                    "No validator found for Verifiable Credential v1.1".to_string(),
                )),
            CredentialTypeVersion::VerifiableCredentialV2 => VALIDATOR_REGISTRY
                .get("VerifiableCredentialV2.json")
                .ok_or(JsonSchemaError::GetCredentialTypeError(
                    "No validator found for Verifiable Credential v2".to_string(),
                )),
            CredentialTypeVersion::EuropeanDigitalCredentialV3_3 => VALIDATOR_REGISTRY
                .get("EuropeanDigitalCredentialV3_3.json")
                .ok_or(JsonSchemaError::GetCredentialTypeError(
                    "No validator found for European Digital Credential v3.3".to_string(),
                )),
            CredentialTypeVersion::OpenBadgeCredentialV3 => {
                VALIDATOR_REGISTRY
                    .get("OpenBadgeCredentialV3.json")
                    .ok_or(JsonSchemaError::GetCredentialTypeError(
                        "No validator found for OpenBadge Credential v3".to_string(),
                    ))
            }
            CredentialTypeVersion::Unknown => Err(JsonSchemaError::GetCredentialTypeError(
                "Unknown Credential Type version, no corresponding validator found".to_string(),
            )),
        }
    }
}

impl CredentialType {
    fn get_version(&self, data: &Value) -> Result<CredentialTypeVersion, JsonSchemaError> {
        let context_array = serde_json::from_value::<Vec<String>>(data["@context"].clone())
            .map_err(|e| JsonSchemaError::InvalidJsonData(e.to_string()))?;

        match self {
            CredentialType::OpenBadgeCredential => {
                match context_array
                    .get(1)
                    .ok_or(JsonSchemaError::GetCredentialTypeError(
                        "Invalid Credential Format: Second context element missing from OpenBadge Credential"
                            .to_string(),
                    ))?
                    .as_str()
                {
                    context
                        if context.starts_with("https://purl.imsglobal.org/spec/ob/v3p0/context-")
                            && context.ends_with(".json") =>
                    {
                        Ok(CredentialTypeVersion::OpenBadgeCredentialV3)
                    }
                    _ => Err(JsonSchemaError::GetCredentialTypeError(
                        "Invalid Credential Format: Unexpected second context element in OpenBadge Credential"
                            .to_string(),
                    )),
                }
            }
            CredentialType::VerifiableCredential => {
                match context_array
                    .first()
                    .ok_or(JsonSchemaError::GetCredentialTypeError(
                        "Invalid Credential Format: Required first context element missing from Verifiable Credential"
                            .to_string(),
                    ))?
                    .as_str()
                {
                    "https://www.w3.org/2018/credentials/v1" => Ok(CredentialTypeVersion::VerifiableCredentialV1_1),
                    "https://www.w3.org/ns/credentials/v2" => Ok(CredentialTypeVersion::VerifiableCredentialV2),
                    _ => Err(JsonSchemaError::GetCredentialTypeError(
                        "Invalid Credential Format: Unexpected first context element in Verifiable Credential"
                            .to_string(),
                    )),
                }
            }
            CredentialType::EuropeanDigitalCredential => {
                // The current provided ELM EDC schema contains no specific context value, only the context value of the VC DM 1.1 it builds upon.
                // Therefore, there is no way to determine the version except for the description.
                // For now we will shortcut this as ELM schemas are still in development and only time will tell the best way to determine versions once multiple schemas are published.
                Ok(CredentialTypeVersion::EuropeanDigitalCredentialV3_3)
            }
            CredentialType::Unknown => {
                warn!("No version found for credential type: {self:?}");
                Ok(CredentialTypeVersion::Unknown)
            }
        }
    }

    pub fn validate(&self, data: &Value) -> Result<(), JsonSchemaError> {
        let version = self.get_version(data)?;

        match version {
            CredentialTypeVersion::Unknown => {
                warn!("Credential Type unknown, skipping validation.");
                Ok(())
            }
            _ => {
                let errors: Vec<_> = version
                    .get_validator()?
                    .iter_errors(data)
                    .map(|e| {
                        format!(
                            "Error: {}\nField: {}\nSchema Path: {}\n",
                            e, // The Display implementation of ValidationError provides a human-readable error message, however it drops a lot of valuable information. Hence, the addition of the subsequent 2 fields.
                            e.instance_path(),
                            e.schema_path()
                        )
                    })
                    .collect();
                if !errors.is_empty() {
                    Err(JsonSchemaError::CredentialValidationError(version.to_string(), errors))
                } else {
                    info!("Credential type: {self:?} successfully validated against corresponding JSON Schema");
                    Ok(())
                }
            }
        }
    }
}

/// Helper function to create the static ref Validators from JSON Schema files.
fn compile_validator(json_schema_key: &str) -> Result<Validator, JsonSchemaError> {
    let json_schema = SCHEMA_REGISTRY.get(json_schema_key).ok_or_else(|| {
        JsonSchemaError::InvalidJsonData("Failed to convert JSON Schema &str to serde_json::Value".to_string())
    })?;

    // The build() function autodetects the Json Schema draft version so no need to set this manually.
    jsonschema::options()
        .with_retriever(MemoryRetriever {})
        .should_validate_formats(true)
        .build(json_schema)
        .map_err(|_| {
            JsonSchemaError::InvalidJsonData(format!(
                "Failed to compile JSON Schema from serde_json::Value: {json_schema}"
            ))
        })
}

/// This struct is solely used to implement the `Retrieve` trait from the `jsonschema` crate,
/// allowing us to reference other JSON Schemas in this folder via $ref in any JSON Schema.
struct MemoryRetriever;

/// Implementation of the `Retrieve` trait for loading local JSON Schema files
impl Retrieve for MemoryRetriever {
    fn retrieve(&self, key: &Uri<String>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let path = key.path();
        let registry_key = path.split('/').next_back().unwrap_or(path).as_str();

        SCHEMA_REGISTRY
            .get(registry_key)
            .cloned()
            .ok_or_else(|| format!("JSON Schema not found for key: {}", key).into())
    }
}

#[derive(Error, Debug)]
pub enum JsonSchemaError {
    #[error("Get Credential Type Error: `{0}`")]
    GetCredentialTypeError(String),
    #[error("Failed to parse JSON data: `{0}`")]
    InvalidJsonData(String),
    #[error("Credential validation failed for type `{0}`:\n{}", format_errors(.1))]
    CredentialValidationError(String, Vec<String>),
}

fn format_errors(errors: &[String]) -> String {
    errors
        .iter()
        .enumerate()
        .map(|(i, e)| format!("  [{}] {}", i + 1, e))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazy_static::lazy_static;
    use serde_json::json;

    lazy_static! {
        static ref EXAMPLE_BASIC_OB3: Value = json!({
            "@context": [
                "https://www.w3.org/ns/credentials/v2",
                "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.3.json"
            ],
            "id": "http://example.com/credentials/3527",
            "type": ["VerifiableCredential", "AchievementCredential"],
            "issuer": {
                "id": "https://example.com/issuers/876543",
                "type": ["Profile"],
                "name": "Example Corp"
            },
            "validFrom": "2010-01-01T00:00:00Z",
            "name": "Teamwork Badge",
            "credentialSubject": {
                "id": "did:example:ebfeb1f712ebc6f1c276e12ec21",
                "type": ["AchievementSubject"],
                "activityStartDate": "2020-01-01T00:00:00Z",
                "activityEndDate": "2020-06-01T00:00:00Z",
                "achievement": {
                    "id": "https://example.com/achievements/21st-century-skills/teamwork",
                    "type": ["Achievement"],
                    "criteria": {
                        "narrative": "Team members are nominated for this badge by their peers and recognized upon review by Example Corp management."
                    },
                    "description": "This badge recognizes the development of the capacity to collaborate within a group environment.",
                    "name": "Teamwork",
                    "fieldOfStudy": "Business",
                    "specialization": "Team Leadership"
                }
            }
        });
        static ref EXAMPLE_BASIC_ELM_EDC: Value = json!({
            "@context": [
                "https://www.w3.org/2018/credentials/v1",
                "https://elm.edc.nl/credentials/v3.3/context.json"
            ],
            "id": "http://example.com/credentials/elm-edc-001",
            "type": [
                "VerifiableCredential",
                "EuropeanDigitalCredential"
            ],
            "name": "ELM EDC Example Credential",
            "issuer": {
                "id": "https://example.com/issuers/123456",
                "type": "Organisation",
                "legalName": { "en": "ELM Example University" },
                "location": {
                "type": "Location",
                "address": {
                    "type": "Address",
                    "countryCode": { "id": "http://publications.europa.eu/resource/authority/country/NLD" }
                }
                }
            },
            "issuanceDate": "2023-01-01T00:00:00Z",
            "issued": "2023-01-01T00:00:00Z",
            "validFrom": "2023-01-01T00:00:00Z",
            "credentialProfiles": [
                { "id": "http://data.europa.eu/snb/model/elm/2.0" }
            ],
            "displayParameter": {
                "type": "DisplayParameter",
                "title": { "en": "Example Credential" },
                "description": { "en": "A demo credential" },
                "language": [ { "id": "http://publications.europa.eu/resource/authority/language/ENG" } ],
                "primaryLanguage": { "id": "http://publications.europa.eu/resource/authority/language/ENG" },
                "individualDisplay": [
                {
                    "type": "IndividualDisplay",
                    "language": {
                        "id": "http://publications.europa.eu/resource/authority/language/ENG",
                        "type": "Concept"
                    },
                    "displayDetail": [
                    {
                        "type": "DisplayDetail",
                        "image": {
                            "type": "MediaObject",
                            "contentType": { "id": "http://publications.europa.eu/resource/authority/file-type/PNG" },
                            "contentEncoding": { "id": "http://publications.europa.eu/resource/authority/encoding/BASE64" },
                            "content": "iVBOR..."
                        },
                        "page": 1
                    }
                    ]
                }
                ]
            },
            "credentialSchema": [{
                "id": "https://elm.edc.nl/credentials/v3.3/schema.json",
                "type": "JsonSchema"
            }],
            "credentialSubject": {
                "id": "did:example:abcdef1234567890",
                "type": "Person",
                "fullName": { "en": "John Doe" }
            }
        });
    }

    #[test]
    fn credential_schema_validation_elm_edc_ok() {
        let cred_type = CredentialType::EuropeanDigitalCredential;
        let result = cred_type.validate(&EXAMPLE_BASIC_ELM_EDC);
        assert!(result.is_ok());
    }

    #[test]
    fn credential_schema_validation_obv3_ok() {
        let cred_type = CredentialType::OpenBadgeCredential;
        let result = cred_type.validate(&EXAMPLE_BASIC_OB3);
        assert!(result.is_ok());
    }

    #[test]
    fn credential_schema_validation_obv3_err() {
        let mut invalid_ob3 = EXAMPLE_BASIC_OB3.clone();

        *invalid_ob3.get_mut("id").unwrap() = json!(["InvalidId"]);
        *invalid_ob3
            .get_mut("credentialSubject")
            .unwrap()
            .get_mut("achievement")
            .unwrap()
            .get_mut("id")
            .unwrap() = json!(["InvalidId"]);

        let cred_type = CredentialType::OpenBadgeCredential;
        let result = cred_type.validate(&invalid_ob3);
        assert!(result.is_err());
    }

    #[test]
    fn credential_schema_validation_unknown_type() {
        let mut invalid_ob3 = EXAMPLE_BASIC_OB3.clone();

        *invalid_ob3.get_mut("type").unwrap() = json!(["UnknownType"]);

        let cred_type = CredentialType::Unknown;
        let result = cred_type.validate(&invalid_ob3);
        assert!(result.is_ok());
    }
}
