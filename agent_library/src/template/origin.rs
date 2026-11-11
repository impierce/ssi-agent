use serde::{Deserialize, Serialize};

use super::{error::TemplateError, public_content::DIGEST_PREFIX};

/// Where a template was imported from.
///
/// Set by the importing caller, stored as given, and never fetched or verified. Compare `digest`
/// with the digest of the source's current public content to tell whether the source changed since
/// the import, and with the digest of the local template's public content to tell whether the copy
/// was edited locally.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateOrigin {
    /// URI of the source, e.g. the identifier of the publishing entity.
    pub source: String,
    /// Identifier of the template at the source. Opaque.
    pub source_template_id: String,
    /// Digest of the template's public content at import time: `sha256-` followed by 64 lowercase
    /// hexadecimal digits.
    pub digest: String,
    /// Whether the importer wants to hear about changes at the source. Off for a copy that is meant to be taken once
    /// and maintained locally. Not part of the template's public content, so it never changes its digest. Absent means
    /// `true`.
    #[serde(default = "check_for_updates_by_default")]
    #[schema(default = true)]
    pub check_for_updates: bool,
}

fn check_for_updates_by_default() -> bool {
    true
}

impl TemplateOrigin {
    /// Checks the format of each field; nothing is fetched.
    pub fn validate(&self) -> Result<(), TemplateError> {
        if !is_absolute_uri(&self.source) {
            return Err(TemplateError::InvalidOrigin(
                "`source` must be an absolute URI".to_string(),
            ));
        }
        if self.source_template_id.trim().is_empty() {
            return Err(TemplateError::InvalidOrigin(
                "`sourceTemplateId` must not be empty".to_string(),
            ));
        }
        if !is_digest(&self.digest) {
            return Err(TemplateError::InvalidOrigin(format!(
                "`digest` must be `{DIGEST_PREFIX}` followed by 64 lowercase hexadecimal digits"
            )));
        }
        Ok(())
    }
}

/// `scheme ":" rest` per RFC 3986, section 3.1, with a non-empty rest, no whitespace, and every `%` starting a
/// percent-encoded octet (section 2.1).
fn is_absolute_uri(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once(':') else {
        return false;
    };
    let mut scheme_chars = scheme.chars();
    scheme_chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme_chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !rest.is_empty()
        && !value.chars().any(char::is_whitespace)
        && has_valid_percent_encoding(rest)
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.iter().enumerate().all(|(index, byte)| {
        *byte != b'%'
            || bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
    })
}

fn is_digest(value: &str) -> bool {
    value
        .strip_prefix(DIGEST_PREFIX)
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    const DIGEST: &str = "sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn origin(source: &str, source_template_id: &str, digest: &str) -> TemplateOrigin {
        TemplateOrigin {
            source: source.to_string(),
            source_template_id: source_template_id.to_string(),
            digest: digest.to_string(),
            check_for_updates: true,
        }
    }

    #[rstest]
    #[case::https("https://issuer.example")]
    #[case::did("did:web:issuer.example")]
    #[case::urn("urn:uuid:0b7c7a46-2f1e-4f0b-9a57-0d7c1f8e2b6a")]
    #[case::percent_encoded("did:web:issuer.example%3A8443")]
    fn accepts_absolute_uri_sources(#[case] source: &str) {
        assert!(origin(source, "0b7c", DIGEST).validate().is_ok());
    }

    #[rstest]
    #[case::empty("")]
    #[case::relative("issuer.example")]
    #[case::no_rest("https:")]
    #[case::scheme_starts_with_digit("1https://issuer.example")]
    #[case::whitespace("https://issuer .example")]
    #[case::invalid_percent_encoding("https://issuer.example/%GG")]
    #[case::truncated_percent_encoding("https://issuer.example/%4")]
    fn rejects_other_sources(#[case] source: &str) {
        assert!(matches!(
            origin(source, "0b7c", DIGEST).validate(),
            Err(TemplateError::InvalidOrigin(_))
        ));
    }

    #[rstest]
    #[case::empty("")]
    #[case::whitespace(" ")]
    fn rejects_empty_source_template_ids(#[case] source_template_id: &str) {
        assert!(matches!(
            origin("https://issuer.example", source_template_id, DIGEST).validate(),
            Err(TemplateError::InvalidOrigin(_))
        ));
    }

    #[rstest]
    #[case::no_prefix("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")]
    #[case::other_algorithm("sha512-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")]
    #[case::uppercase("sha256-0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef")]
    #[case::too_short("sha256-0123456789abcdef")]
    #[case::too_long("sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef00")]
    #[case::base64("sha256-ASNFZ4mrze8BI0VniavN7wEjRWeJq83vASNFZ4mrze8=")]
    fn rejects_malformed_digests(#[case] digest: &str) {
        assert!(matches!(
            origin("https://issuer.example", "0b7c", digest).validate(),
            Err(TemplateError::InvalidOrigin(_))
        ));
    }

    #[test]
    fn serializes_in_camel_case() {
        assert_eq!(
            serde_json::to_value(origin("https://issuer.example", "0b7c", DIGEST)).unwrap(),
            serde_json::json!({
                "source": "https://issuer.example",
                "sourceTemplateId": "0b7c",
                "digest": DIGEST,
                "checkForUpdates": true,
            })
        );
    }

    #[test]
    fn checks_for_updates_unless_told_otherwise() {
        let stored: TemplateOrigin = serde_json::from_value(serde_json::json!({
            "source": "https://issuer.example",
            "sourceTemplateId": "0b7c",
            "digest": DIGEST,
        }))
        .unwrap();
        assert!(stored.check_for_updates);

        let opted_out = TemplateOrigin {
            check_for_updates: false,
            ..origin("https://issuer.example", "0b7c", DIGEST)
        };
        let round_trip: TemplateOrigin = serde_json::from_value(serde_json::to_value(&opted_out).unwrap()).unwrap();
        assert_eq!(round_trip, opted_out);
    }
}
