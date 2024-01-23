use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum IdentityManagerCommand {
    /// Tries to load an existing identity for the given method (such as `did:key`). If it cannot be found, it will try to create it.
    LoadIdentity { method: Method },
    /// Destroys the managed identity for the given method and will return the funds to the initial donating address.
    DestroyIdentity { method: Method },
}

/// Supported DID methods
#[derive(Serialize, Deserialize, Debug)]
#[allow(non_camel_case_types)]
pub enum Method {
    key,
    web,
    iota,
    #[serde(rename = "iota:rms")]
    iota_rms,
}
