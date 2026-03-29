use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub enum DataAccessConsentTokenCommand {
    StoreDataAccessConsentToken { id: String, token: String },
}
