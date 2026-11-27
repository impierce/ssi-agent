//! What a key id names in STACKIT KMS, and which labels can name a key.

use identity_kit::keys::KeyId;
use uuid::Uuid;

/// STACKIT's limit on the length of a display name.
const MAX_LABEL: usize = 64;

/// What a [`KeyId`] names in a key ring.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyName<'a> {
    /// `<key id>/<version>`: one version of a key, for good.
    Version { key_id: &'a str, number: u64 },
    /// `<key id>`: a key, signing with its newest usable version.
    Key(&'a str),
    /// Anything else: the key that has this display name and isn't deleted, signing with its newest usable version.
    Label(&'a str),
}

impl<'a> KeyName<'a> {
    pub(crate) fn parse(id: &'a KeyId) -> Self {
        let id = id.as_str();
        if is_key_id(id) {
            return Self::Key(id);
        }
        match id.split_once('/') {
            Some((key_id, number)) if is_key_id(key_id) => match number.parse() {
                Ok(number) => Self::Version { key_id, number },
                Err(_) => Self::Label(id),
            },
            _ => Self::Label(id),
        }
    }

    /// The key id this names the key by, or the label.
    pub(crate) fn key(&self) -> &'a str {
        match self {
            Self::Version { key_id, .. } | Self::Key(key_id) => key_id,
            Self::Label(label) => label,
        }
    }
}

fn is_key_id(id: &str) -> bool {
    Uuid::try_parse(id).is_ok()
}

/// Why `label` can't name a key, or `None` when it can: STACKIT takes up to 64 letters, digits, `_` and `-` as a
/// display name, and a UUID would read as a key id.
pub(crate) fn label_refusal(label: &str) -> Option<String> {
    let reason = if label.is_empty()
        || !label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        "use letters, digits, `_` and `-`"
    } else if label.len() > MAX_LABEL {
        "a display name has at most 64 characters"
    } else if is_key_id(label) {
        "it reads as a key id"
    } else {
        return None;
    };
    Some(format!("`{label}` can't name a STACKIT KMS key: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_ID: &str = "0b6a7f8e-3f4c-4d2a-9b1e-5c8d7e6f5a4b";

    #[test]
    fn names_a_key_by_its_id() {
        let id = KeyId::new(KEY_ID);

        assert_eq!(KeyName::parse(&id), KeyName::Key(KEY_ID));
    }

    #[test]
    fn names_a_version_of_a_key() {
        let id = KeyId::new(format!("{KEY_ID}/2"));

        assert_eq!(
            KeyName::parse(&id),
            KeyName::Version {
                key_id: KEY_ID,
                number: 2
            }
        );
    }

    #[test]
    fn names_a_key_by_its_label() {
        let id = KeyId::new("issuer");

        assert_eq!(KeyName::parse(&id), KeyName::Label("issuer"));
    }

    #[test]
    fn accepts_a_label_of_letters_digits_underscores_and_hyphens() {
        assert_eq!(label_refusal("Issuer_2026-eu"), None);
    }

    #[test]
    fn refuses_a_label_with_a_slash() {
        assert_eq!(
            label_refusal("issuer/eu"),
            Some("`issuer/eu` can't name a STACKIT KMS key: use letters, digits, `_` and `-`".to_string())
        );
    }

    #[test]
    fn refuses_a_label_longer_than_stackit_accepts() {
        let label = "a".repeat(65);

        assert_eq!(
            label_refusal(&label),
            Some(format!(
                "`{label}` can't name a STACKIT KMS key: a display name has at most 64 characters"
            ))
        );
    }

    #[test]
    fn refuses_a_label_that_reads_as_a_key_id() {
        assert_eq!(
            label_refusal(KEY_ID),
            Some(format!("`{KEY_ID}` can't name a STACKIT KMS key: it reads as a key id"))
        );
    }
}
