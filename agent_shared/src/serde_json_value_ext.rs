use serde_json::Value;

// Helper methods to simplify working with serde_json::Value.
pub trait SerdeJsonValueExt {
    /// Inserts a value at the specified path, creating intermediate objects as needed.
    /// The path includes the final key name where the value will be inserted.
    /// For example, to set `$.issuer.id = "123"`, use:
    /// `credential.insert_at_path(&["issuer", "id"], json!("123"))`
    ///
    /// Returns `Some(&mut self)` on success, `None` on failure.
    fn insert_at_path(&mut self, path: &[&str], value: serde_json::Value) -> Option<&mut Self>;

    /// This method is the same as `insert_at_path` but it only inserts the value if there is no value already present at the path.
    fn insert_if_none(&mut self, path: &[&str], value: serde_json::Value) -> Option<&mut Self>;

    fn to_unescaped_string(&self) -> Option<String>;
}

impl SerdeJsonValueExt for serde_json::Value {
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

    /// Helper to convert a `serde_json::Value` to an owned `Option<String>`.
    ///
    /// This resolves two common inconveniences:
    /// 1. `to_string()` serializes the JSON, resulting in extra quotes (e.g., "\"value\"").
    /// 2. `as_str()` returns a `&str`, but often an owned `String` is required.
    ///
    /// Returns `Some(String)` if the value is a JSON string, or `None` otherwise.
    fn to_unescaped_string(&self) -> Option<String> {
        self.as_str().map(ToString::to_string)
    }
}

#[cfg(test)]
pub mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_insert_at_path() {
        let mut value = json!({});
        value.insert_at_path(&["a", "b", "c"], json!(42)).unwrap();
        assert_eq!(value, json!({"a": {"b": {"c": 42}}}));
    }

    #[test]
    fn test_insert_if_none() {
        let mut value = json!({"a": {"b": {"c": 42}}});
        // Attempt to insert at an existing path - should not overwrite
        value.insert_if_none(&["a", "b", "c"], json!(100)).unwrap();
        assert_eq!(value, json!({"a": {"b": {"c": 42}}}));

        // Insert at a new path - should succeed
        value.insert_if_none(&["a", "b", "d"], json!(100)).unwrap();
        assert_eq!(value, json!({"a": {"b": {"c": 42, "d": 100}}}));
    }

    #[test]
    fn test_to_unescaped_string() {
        let value = json!("Hello, World!");
        // value.to_string() would return "\"Hello, World!\"", showcasing our inconvenience.
        assert_eq!(value.to_string(), "\"Hello, World!\"".to_string());

        // This is the result we want, giving us the unescaped string directly from the Value or a None which we can properly error handle.
        assert_eq!(value.to_unescaped_string(), Some("Hello, World!".to_string()));

        let non_string_value = json!(42);
        assert_eq!(non_string_value.to_unescaped_string(), None);
    }
}
