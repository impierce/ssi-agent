use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub enum DataAccessConsentTokenCommand {
    StoreDataAccessConsentToken { id: String, token: String },
    // This command is only for the purpose of storing the event, no new or updated data needs to be stored.
    ResolveDataAccessConsentToken { id: String, called_endpoint: String },
}
