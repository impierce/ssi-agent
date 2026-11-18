use crate::credential::aggregate::CredentialExpiry;
use agent_library::{
    queries,
    state::LibraryState,
    template::aggregate::{Expiration, Status, Template},
};
use agent_shared::handlers::QueryHandlerError;
use serde_json::Value;
use shared_kernel::authorization::Caller;

/// One violation of the template schema by a credential's claims.
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct SchemaViolation {
    /// JSON pointer to the offending value within the claims; empty for the claims object itself.
    pub path: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateIssuanceError {
    #[error(transparent)]
    Query(#[from] QueryHandlerError),
    #[error("The template identifier must not be empty")]
    MissingTemplateId,
    #[error("No template found with id: `{0}`")]
    TemplateNotFound(String),
    #[error("Credential issuance requires the template to be Published.")]
    TemplateNotPublished,
    #[error("Invalid template expiration: {0}")]
    InvalidExpiration(String),
    #[error("The template requires an expiration date not after {0}")]
    ExpirationExceedsTemplateLimit(String),
    #[error("The template's schema is not a valid JSON Schema: {0}")]
    InvalidSchema(String),
    #[error("The credential does not match the template schema. Violations:\n{descriptions}")]
    SchemaValidation {
        violations: Vec<SchemaViolation>,
        descriptions: String,
    },
}

pub async fn published_template(
    state: &LibraryState,
    caller: Caller,
    template_id: &str,
) -> Result<Template, TemplateIssuanceError> {
    if template_id.is_empty() {
        return Err(TemplateIssuanceError::MissingTemplateId);
    }
    let template = queries::get_template(state, caller, template_id)
        .await?
        .ok_or_else(|| TemplateIssuanceError::TemplateNotFound(template_id.to_owned()))?;
    if template.status != Status::Published {
        return Err(TemplateIssuanceError::TemplateNotPublished);
    }
    Ok(template)
}

pub fn validate_credential_against_template(
    credential: &Value,
    template: &Template,
) -> Result<(), TemplateIssuanceError> {
    if let Some(schema) = template.schema.as_ref() {
        let data = credential.get("credentialSubject").unwrap_or(credential);
        let validator =
            jsonschema::validator_for(schema).map_err(|e| TemplateIssuanceError::InvalidSchema(e.to_string()))?;
        let (violations, descriptions): (Vec<SchemaViolation>, Vec<String>) = validator
            .iter_errors(data)
            .enumerate()
            .map(|(i, e)| {
                (
                    SchemaViolation {
                        path: e.instance_path().to_string(),
                        message: e.to_string(),
                    },
                    format!(
                        "  [{}] Path `{}`: {} (schema path: {})",
                        i + 1,
                        e.instance_path(),
                        e,
                        e.schema_path()
                    ),
                )
            })
            .unzip();
        if !violations.is_empty() {
            return Err(TemplateIssuanceError::SchemaValidation {
                violations,
                descriptions: descriptions.join("\n"),
            });
        }
    }
    Ok(())
}

pub fn template_credential_expiry(
    template: &Template,
    explicit: Option<CredentialExpiry>,
) -> Result<CredentialExpiry, TemplateIssuanceError> {
    let deadline = expiration_to_credential_expiry(&template.credential_expiration)?;
    if let Some(explicit) = explicit {
        validate_expiry_within_template_deadline(&explicit, &deadline)?;
        Ok(explicit)
    } else {
        Ok(deadline)
    }
}

pub fn expiration_to_credential_expiry(expiration: &Expiration) -> Result<CredentialExpiry, TemplateIssuanceError> {
    match expiration {
        Expiration::Never => Ok(CredentialExpiry::Never),
        Expiration::DateTime(s) => chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| CredentialExpiry::Fixed(dt.with_timezone(&chrono::Utc)))
            .map_err(|e| TemplateIssuanceError::InvalidExpiration(format!("{s}: {e}"))),
        Expiration::Duration(s) => iso8601::duration(s)
            .map_err(|_| TemplateIssuanceError::InvalidExpiration(s.clone()))
            .map(|d| {
                let delta = match d {
                    iso8601::Duration::YMDHMS {
                        year,
                        month,
                        day,
                        hour,
                        minute,
                        second,
                        millisecond,
                    } => {
                        chrono::Duration::days(year as i64 * 365 + month as i64 * 30 + day as i64)
                            + chrono::Duration::hours(hour as i64)
                            + chrono::Duration::minutes(minute as i64)
                            + chrono::Duration::seconds(second as i64)
                            + chrono::Duration::milliseconds(millisecond as i64)
                    }
                    iso8601::Duration::Weeks(w) => chrono::Duration::weeks(w as i64),
                };
                CredentialExpiry::Fixed(chrono::Utc::now() + delta)
            }),
    }
}

pub fn validate_expiry_within_template_deadline(
    explicit: &CredentialExpiry,
    deadline: &CredentialExpiry,
) -> Result<(), TemplateIssuanceError> {
    match (explicit, deadline) {
        (_, CredentialExpiry::Never) => Ok(()),
        (CredentialExpiry::Fixed(requested), CredentialExpiry::Fixed(limit)) if requested <= limit => Ok(()),
        (_, CredentialExpiry::Fixed(limit)) => Err(TemplateIssuanceError::ExpirationExceedsTemplateLimit(
            limit.to_rfc3339(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_library::template::aggregate::test_utils::{structured_claims, structured_schema};
    use rstest::rstest;
    use serde_json::json;

    fn structured_template() -> Template {
        Template {
            schema: Box::new(Some(structured_schema())),
            ..Default::default()
        }
    }

    #[rstest]
    #[case::flat(structured_claims())]
    #[case::under_credential_subject(json!({ "credentialSubject": structured_claims() }))]
    fn claims_with_nested_objects_lists_and_tables_satisfy_their_template(#[case] credential: Value) {
        assert!(validate_credential_against_template(&credential, &structured_template()).is_ok());
    }

    /// A violation inside an object, a list or a table names the place it is at.
    #[rstest]
    #[case::required_in_object(|claims: &mut Value| { claims["Address"].as_object_mut().unwrap().remove("Street"); }, "/Address")]
    #[case::type_in_nested_object(|claims: &mut Value| claims["Address"]["Coordinates"]["Latitude"] = json!(51.89), "/Address/Coordinates/Latitude")]
    #[case::empty_required_list(|claims: &mut Value| claims["Skills"] = json!([]), "/Skills")]
    #[case::too_many_items(|claims: &mut Value| claims["Skills"] = json!(["a", "b", "c", "d", "e", "f"]), "/Skills")]
    #[case::type_of_list_item(|claims: &mut Value| claims["Skills"][1] = json!(7), "/Skills/1")]
    #[case::fixed_list(|claims: &mut Value| claims["Accreditations"] = json!(["https://other.example.com"]), "/Accreditations")]
    #[case::required_in_list_item(|claims: &mut Value| { claims["Work experience"][1].as_object_mut().unwrap().remove("Employer"); }, "/Work experience/1")]
    #[case::type_in_list_in_list_item(|claims: &mut Value| claims["Work experience"][0]["Roles"][0] = json!(true), "/Work experience/0/Roles/0")]
    #[case::type_of_table_cell(|claims: &mut Value| claims["Completed classes"][1]["Code"] = json!(1029), "/Completed classes/1/Code")]
    #[case::table_that_is_no_array(|claims: &mut Value| claims["Completed classes"] = json!({ "Code": "K0303" }), "/Completed classes")]
    fn violation_in_nested_claims_names_its_path(#[case] break_claims: fn(&mut Value), #[case] path: &str) {
        let mut claims = structured_claims();
        break_claims(&mut claims);

        let Err(TemplateIssuanceError::SchemaValidation { violations, .. }) =
            validate_credential_against_template(&claims, &structured_template())
        else {
            panic!("the claims were accepted");
        };

        let paths: Vec<&str> = violations.iter().map(|violation| violation.path.as_str()).collect();
        assert_eq!(paths, vec![path]);
    }
}
