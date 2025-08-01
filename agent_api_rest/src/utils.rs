/// A helper module for deserializing fields that can be present with a value,
/// present and explicitly `null`, or not present at all.
pub(crate) mod serde_explicit_null {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Deserializes a field that is present, wrapping it in `Some`.
    /// This allows us to distinguish between `{"key": null}` (`Some(None)`)
    /// and `{}` (`None`).
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        // Deserialize the inner value (which could be `T` or `null`) into an `Option<T>`.
        let inner = Option::<T>::deserialize(deserializer)?;
        // Wrap the result in `Some` to indicate the key was present in the JSON.
        Ok(Some(inner))
    }

    /// Serializes the inner `Option<T>` of `Option<Option<T>>`.
    /// This should be used with `#[serde(skip_serializing_if = "Option::is_none")]`
    /// on the field to omit it when the outer `Option` is `None`.
    pub fn serialize<S, T>(value: &Option<Option<T>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        match value {
            Some(inner) => inner.serialize(serializer),
            // This branch is technically unreachable if `skip_serializing_if` is used,
            // but we handle it for completeness.
            None => serializer.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::serde_explicit_null;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Deserialize, Serialize, Debug, PartialEq)]
    struct TestStruct {
        #[serde(default, with = "serde_explicit_null")]
        key: Option<Option<String>>,
    }

    #[test]
    fn test_deserialize_explicit_null() {
        let json_with_value = json!({ "key": "value" });
        let json_with_null = json!({ "key": null });
        let json_without_key = json!({});

        let result_with_value: TestStruct = serde_json::from_value(json_with_value).unwrap();
        let result_with_null: TestStruct = serde_json::from_value(json_with_null).unwrap();
        let result_without_key: TestStruct = serde_json::from_value(json_without_key).unwrap();

        assert_eq!(result_with_value.key, Some(Some("value".to_string())));
        assert_eq!(result_with_null.key, Some(None));
        assert_eq!(result_without_key.key, None);
    }
}
