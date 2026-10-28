use serde::{de::DeserializeOwned, Serialize};

// TODO: remove once nothing converts between IdentityKit's and the `identity_*` crates' types anymore: the IOTA
// document handling in `document::aggregate`.
/// Converts between IdentityKit's DID types and the `identity_*` crates' ones, for the code that still uses the
/// latter. Both read and write the JSON representation of DID Core, so the conversion goes through it.
pub fn convert<T: DeserializeOwned>(value: &impl Serialize) -> Result<T, serde_json::Error> {
    serde_json::to_value(value).and_then(serde_json::from_value)
}
