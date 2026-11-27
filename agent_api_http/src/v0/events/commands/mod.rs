pub mod activate_event_publisher;
pub mod create_event_publisher;
pub mod deactivate_event_publisher;
pub mod delete_event_publisher;
pub mod update_event_publisher;

// TODO: have a second look at this to see if we really need this, or if we can just use the `uuid` crate to validate the id.
pub(super) fn deserialize_non_empty_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let id: String = serde::Deserialize::deserialize(deserializer)?;
    if id.is_empty() || id.contains('\0') {
        return Err(serde::de::Error::custom("event publisher id must be non-empty"));
    }
    Ok(id)
}
