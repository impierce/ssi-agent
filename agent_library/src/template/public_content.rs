use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::aggregate::{DataModel, Display, Expiration, HolderType, PropertyAttribute, Template};

/// Prefix of a [`PublicTemplateContent::digest`], naming its hash algorithm.
pub const DIGEST_PREFIX: &str = "sha256-";

/// Public content of a template: the fields of the public template projection
/// (`GET /public/templates`) without `modifiedAt`.
///
/// Unknown fields are ignored on deserialization, so an entry of a source's public template
/// projection, `modifiedAt` included, deserializes into its public content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PublicTemplateContent {
    pub title: String,
    pub description: Option<String>,
    pub display: Display,
    pub data_model: DataModel,
    pub holder_type: HolderType,
    pub r#type: Vec<String>,
    pub tags: Option<Vec<String>>,
    pub credential_expiration: Expiration,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
}

impl PublicTemplateContent {
    /// Digest for change detection: `sha256-` followed by the lowercase hexadecimal SHA-256 of the
    /// RFC 8785 (JCS) canonical JSON of this content.
    ///
    /// Top-level members whose value is `null` are left out, so an absent optional field and a
    /// `null` one give the same digest, and adding an optional field keeps the digests of content
    /// that does not set it.
    pub fn digest(&self) -> String {
        let hash = Sha256::digest(self.canonical_json());
        let mut digest = String::with_capacity(DIGEST_PREFIX.len() + 2 * hash.len());
        digest.push_str(DIGEST_PREFIX);
        for byte in hash {
            digest.push(char::from(HEX[usize::from(byte >> 4)]));
            digest.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        digest
    }

    pub(crate) fn canonical_json(&self) -> Vec<u8> {
        // Neither step can fail: every map key is a string, and `serde_json::Value` holds no
        // non-finite numbers.
        let mut value = serde_json::to_value(self).expect("public template content serializes to JSON");
        if let Some(members) = value.as_object_mut() {
            members.retain(|_, member| !member.is_null());
        }
        serde_json_canonicalizer::to_vec(&value).expect("JSON value canonicalizes")
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

impl From<Template> for PublicTemplateContent {
    fn from(template: Template) -> Self {
        let display = Display::resolve(template.display, &template.title);

        Self {
            title: template.title,
            description: template.description,
            display,
            data_model: template.data_model,
            holder_type: template.holder_type,
            r#type: template.r#type,
            tags: template.tags,
            credential_expiration: template.credential_expiration,
            schema: *template.schema,
            schema_properties_attributes: template.schema_properties_attributes,
        }
    }
}

impl Template {
    /// Public content of this template, as the public template projection shows it.
    pub fn public_content(&self) -> PublicTemplateContent {
        PublicTemplateContent::from(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::aggregate::{FormFieldType, Logo};
    use serde_json::json;

    fn minimal() -> PublicTemplateContent {
        PublicTemplateContent {
            title: "Diploma".to_string(),
            description: None,
            display: Display {
                name: "Diploma".to_string(),
                logo: None,
            },
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            r#type: vec!["VerifiableCredential".to_string()],
            tags: None,
            credential_expiration: Expiration::Never,
            schema: None,
            schema_properties_attributes: None,
        }
    }

    fn canonical(content: &PublicTemplateContent) -> String {
        String::from_utf8(content.canonical_json()).unwrap()
    }

    fn from_json(json: &str) -> PublicTemplateContent {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn digest_of_minimal_content() {
        let content = minimal();

        assert_eq!(
            canonical(&content),
            r#"{"credentialExpiration":{"type":"never"},"dataModel":"w3c_vc_data_model_v2-0","display":{"name":"Diploma"},"holderType":"individual","title":"Diploma","type":["VerifiableCredential"]}"#
        );
        assert_eq!(
            content.digest(),
            "sha256-7859a73aacae9600346ac67660961e4b5a0df706aabe8fd3893a744e8338a31f"
        );
    }

    #[test]
    fn digest_of_full_content() {
        let content = PublicTemplateContent {
            title: "Diploma".to_string(),
            description: Some("Awarded on graduation".to_string()),
            display: Display {
                name: "University Diploma".to_string(),
                logo: Some(Logo {
                    uri: "https://issuer.example/logo.png".to_string(),
                    alt_text: Some("Logo".to_string()),
                }),
            },
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Organization,
            r#type: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
            tags: Some(vec!["education".to_string(), "diploma".to_string()]),
            credential_expiration: Expiration::Duration("P1Y".to_string()),
            schema: Some(json!({ "type": "object", "properties": { "country": { "type": "string" } } })),
            schema_properties_attributes: Some(HashMap::from([
                (
                    "country".to_string(),
                    PropertyAttribute {
                        selectively_disclosable: true,
                        non_removable: false,
                        r#type: Some(FormFieldType::Country),
                    },
                ),
                ("name".to_string(), PropertyAttribute::new(false, true)),
            ])),
        };

        assert_eq!(
            canonical(&content),
            concat!(
                r#"{"credentialExpiration":{"type":"duration","value":"P1Y"},"dataModel":"open_badges_3-0","#,
                r#""description":"Awarded on graduation","#,
                r#""display":{"logo":{"alt_text":"Logo","uri":"https://issuer.example/logo.png"},"name":"University Diploma"},"#,
                r#""holderType":"organization","#,
                r#""schema":{"properties":{"country":{"type":"string"}},"type":"object"},"#,
                r#""schemaPropertiesAttributes":{"country":{"nonRemovable":false,"selectivelyDisclosable":true,"type":"country"},"#,
                r#""name":{"nonRemovable":true,"selectivelyDisclosable":false}},"#,
                r#""tags":["education","diploma"],"title":"Diploma","type":["VerifiableCredential","OpenBadgeCredential"]}"#,
            )
        );
        assert_eq!(
            content.digest(),
            "sha256-762e4876f5aac75685c2246cad8b63fc036899fbd93d86251e349c630baaa7e9"
        );
    }

    #[test]
    fn digest_ignores_key_order_and_whitespace() {
        let compact = from_json(
            r#"{"title":"Diploma","display":{"name":"Diploma"},"dataModel":"w3c_vc_data_model_v2-0","holderType":"individual","type":["VerifiableCredential"],"credentialExpiration":{"type":"never"},"schema":{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"number"}}}}"#,
        );
        let reordered = from_json(
            r#"
            {
                "schema": {
                    "properties": { "b": { "type": "number" }, "a": { "type": "string" } },
                    "type": "object"
                },
                "credentialExpiration": { "type": "never" },
                "type": [ "VerifiableCredential" ],
                "holderType": "individual",
                "dataModel": "w3c_vc_data_model_v2-0",
                "display": { "name": "Diploma" },
                "title": "Diploma"
            }
            "#,
        );

        assert_eq!(compact.digest(), reordered.digest());
    }

    #[test]
    fn digest_keeps_array_order() {
        let mut swapped = minimal();
        swapped.r#type = vec!["VerifiableCredential".to_string(), "Diploma".to_string()];
        let mut original = minimal();
        original.r#type = vec!["Diploma".to_string(), "VerifiableCredential".to_string()];

        assert_ne!(original.digest(), swapped.digest());
    }

    #[test]
    fn digest_sorts_keys_by_utf16_code_units() {
        // The keys of the RFC 8785 sorting example, section 3.2.3: U+1F600 sorts before U+FB33 in
        // UTF-16, but after it in UTF-8 and code point order.
        let mut content = minimal();
        content.schema = Some(json!({
            "properties": {
                "\u{20ac}": {},
                "\r": {},
                "\u{fb33}": {},
                "1": {},
                "\u{1f600}": {},
                "\u{80}": {},
                "\u{f6}": {},
            }
        }));

        let canonical = canonical(&content);

        assert!(
            canonical.contains("{\"properties\":{\"\\r\":{},\"1\":{},\"\u{80}\":{},\"\u{f6}\":{},\"\u{20ac}\":{},\"\u{1f600}\":{},\"\u{fb33}\":{}}}"),
            "{canonical}"
        );
        assert_eq!(
            content.digest(),
            "sha256-930c96b97ee0ca5fe56d1c9b407f661fc03f72b2919a9dc3c045b3c8bb6ae686"
        );
    }

    #[test]
    fn digest_writes_unicode_verbatim_and_escapes_only_what_json_requires() {
        let mut content = minimal();
        content.title = "Zertifikat f\u{fc}r \u{dc}bersetzer \u{1f393}".to_string();
        content.description =
            Some("tab\t, quote\", backslash\\, unit separator\u{1f}, line separator\u{2028}".to_string());

        let canonical = canonical(&content);

        assert!(
            canonical.contains("\"title\":\"Zertifikat f\u{fc}r \u{dc}bersetzer \u{1f393}\""),
            "{canonical}"
        );
        assert!(
            canonical.contains(
                "\"description\":\"tab\\t, quote\\\", backslash\\\\, unit separator\\u001f, line separator\u{2028}\""
            ),
            "{canonical}"
        );
        assert_eq!(
            content.digest(),
            "sha256-0ee1d5871a1562b0da222daf84f0e43e3c49bec85d1780fb1eeba0d1a29534cd"
        );
    }

    #[test]
    fn digest_writes_numbers_like_ecmascript() {
        let content = from_json(
            r#"{"title":"Diploma","display":{"name":"Diploma"},"dataModel":"w3c_vc_data_model_v2-0","holderType":"individual","type":["VerifiableCredential"],"credentialExpiration":{"type":"never"},
                "schema":{"type":"object","properties":{"grade":{"type":"number","minimum":1.0,"maximum":10E0,"multipleOf":0.10,"default":-0.0,"examples":[1e21,1e-7,0.000001,123456789012,4.50]}}}}"#,
        );

        let canonical = canonical(&content);

        assert!(
            canonical.contains(r#"{"default":0,"examples":[1e+21,1e-7,0.000001,123456789012,4.5],"maximum":10,"minimum":1,"multipleOf":0.1,"type":"number"}"#),
            "{canonical}"
        );
        assert_eq!(
            content.digest(),
            "sha256-4f882916931e16a2684f91dfaa0b7475e0a11321aee2889847d5ab1117b97eb3"
        );
    }

    #[test]
    fn digest_treats_absent_and_null_optional_fields_alike() {
        let absent = from_json(
            r#"{"title":"Diploma","display":{"name":"Diploma"},"dataModel":"w3c_vc_data_model_v2-0","holderType":"individual","type":["VerifiableCredential"],"credentialExpiration":{"type":"never"}}"#,
        );
        let null = from_json(
            r#"{"title":"Diploma","description":null,"display":{"name":"Diploma"},"dataModel":"w3c_vc_data_model_v2-0","holderType":"individual","type":["VerifiableCredential"],"tags":null,"credentialExpiration":{"type":"never"},"schema":null,"schemaPropertiesAttributes":null}"#,
        );

        assert_eq!(absent.digest(), minimal().digest());
        assert_eq!(null.digest(), minimal().digest());
    }

    #[test]
    fn digest_keeps_nulls_inside_the_schema() {
        let mut with_null = minimal();
        with_null.schema = Some(json!({ "type": "object", "default": null }));
        let mut without_null = minimal();
        without_null.schema = Some(json!({ "type": "object" }));

        assert_ne!(with_null.digest(), without_null.digest());
    }

    #[test]
    fn digest_changes_with_any_content_field() {
        let original = minimal().digest();
        let changes: Vec<fn(&mut PublicTemplateContent)> = vec![
            |c| c.title = "Certificate".to_string(),
            |c| c.description = Some("Description".to_string()),
            |c| c.display.name = "Certificate".to_string(),
            |c| c.data_model = DataModel::W3CVcDataModelV1_1,
            |c| c.holder_type = HolderType::Organization,
            |c| c.r#type.push("Diploma".to_string()),
            |c| c.tags = Some(vec!["education".to_string()]),
            |c| c.credential_expiration = Expiration::default(),
            |c| c.schema = Some(json!({ "type": "object" })),
            |c| c.schema_properties_attributes = Some(HashMap::new()),
        ];

        for change in changes {
            let mut content = minimal();
            change(&mut content);
            assert_ne!(content.digest(), original, "{content:?}");
        }
    }

    #[test]
    fn public_projection_with_modified_at_deserializes_into_its_content() {
        let projection = r#"{"title":"Diploma","description":null,"display":{"name":"Diploma"},"dataModel":"w3c_vc_data_model_v2-0","holderType":"individual","type":["VerifiableCredential"],"tags":null,"credentialExpiration":{"type":"never"},"schema":null,"schemaPropertiesAttributes":null,"modifiedAt":"2026-01-01T00:00:00Z"}"#;

        assert_eq!(from_json(projection), minimal());
    }

    #[test]
    fn public_content_of_a_template_resolves_the_display_name() {
        let template = Template {
            title: "Diploma".to_string(),
            display: None,
            r#type: vec!["VerifiableCredential".to_string()],
            credential_expiration: Expiration::Never,
            modified_at: Some("2026-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };

        assert_eq!(template.public_content(), minimal());

        let template = Template {
            display: Some(Display {
                name: " ".to_string(),
                logo: None,
            }),
            ..template
        };

        assert_eq!(template.public_content(), minimal());
    }
}
