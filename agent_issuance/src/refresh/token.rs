use agent_shared::{config::config, UrlAppendHelpers as _};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;

/// The secret a refreshable credential carries in its `refreshService`, and the only authorization of a refresh.
///
/// 256 random bits, base64url without padding. UniCore keeps only its SHA-256, as the identifier of the refresh
/// capability it unlocks; `Debug` never prints it.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct RefreshToken(String);

impl RefreshToken {
    /// A fresh token from the operating system's random number generator.
    pub fn generate() -> Self {
        let bytes: [u8; 32] = rand::rng().random();
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// The identifier of the capability this token unlocks: the lowercase hexadecimal SHA-256 of the token.
    pub fn capability_id(&self) -> String {
        Sha256::digest(self.0.as_bytes())
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }

    /// The token itself, for embedding it in a credential. Never log it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RefreshToken {
    /// A token as a holder presents it.
    fn from(token: String) -> Self {
        Self(token)
    }
}

impl std::fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RefreshToken(***)")
    }
}

/// The `refreshService` an opted-in credential embeds at the root of its content: where and with what a holder
/// asks for a fresh credential. `type` is the configuration's refresh service type and `url` the public
/// `/v0/refresh-credential` endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRefreshService {
    #[serde(rename = "type")]
    pub type_: String,
    pub url: String,
    #[schema(value_type = String)]
    pub refresh_token: RefreshToken,
}

impl CredentialRefreshService {
    /// The refresh service of `type_`, at this deployment's public URL, carrying `token`.
    pub fn new(type_: String, token: &RefreshToken) -> Self {
        Self {
            type_,
            url: refresh_credential_url(),
            refresh_token: token.clone(),
        }
    }
}

/// This deployment's public `/v0/refresh-credential` endpoint, the `url` of every refresh service it embeds.
pub fn refresh_credential_url() -> String {
    config()
        .public_url
        .append_path_segment("v0/refresh-credential")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capability_is_keyed_by_the_hash_of_its_token() {
        let token = RefreshToken::from("refresh-token".to_owned());

        assert_eq!(
            token.capability_id(),
            "0eb17643d4e9261163783a420859c92c7d212fa9624106a12b510afbec266120"
        );
    }
}
