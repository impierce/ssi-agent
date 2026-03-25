use async_trait::async_trait;
use identity_core::convert::{FromJson as _, ToJson as _};
use jsonwebtoken::{decode_header, jwk::Jwk as JsonWebTokenJwk, DecodingKey};
use oauth_tsl::{
    relying_party::{decompress_gzip, decrypt_status_list_token, StatusListTokenResponseType},
    status_list::{StatusList, StatusType},
};
use oid4vc_core::{
    credential_status_verifier::CredentialStatusVerifier, verification_material_resolver::VerificationMaterialResolver,
};
use reqwest::{header, redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tracing::{info, warn};
use url::Url;

#[derive(Error, Debug)]
pub enum CredentialStatusCheckerError {
    #[error("Failed to get credential status: {0}")]
    FailedToGetCredentialStatus(String),
    #[error("Credential status is invalid")]
    CredentialStatusInvalid,
}

pub struct CredentialStatusChecker {
    pub verification_material_resolver: Arc<dyn VerificationMaterialResolver>,
}

#[async_trait]
impl CredentialStatusVerifier for CredentialStatusChecker {
    // TODO: in the future it would be nice to have a flag every time it's called or only in the config to choose between strict validation also failing on operational errors or lenient validation as is currently.
    /// This function is basically a wrapper around `verify_status_claim` which allows us to differentiate between operational errors (unable to fetch or decode the status list token) and actual validation failures (credential status is invalid).
    async fn check_credential_status(&self, status_claim: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        match self.verify_status_claim(status_claim).await {
            Ok(_) => Ok(()),
            // Unable to verify status is not a validation failure, so it's logged as a warning and the validation is skipped.
            Err(CredentialStatusCheckerError::FailedToGetCredentialStatus(e)) => {
                warn!("Unable to verify credential status, skipping status check: {}", e);
                Ok(())
            }
            // Actual validation failures propagate as errors
            Err(e) => Err(Box::new(e)),
        }
    }
}

impl CredentialStatusChecker {
    /// Internal helper which does the following steps, as per the OAuth Token Status List specification:
    /// 1. Tries to parse the `status` claim from the credential
    /// 2. Fetch the status list token JWT
    /// 3. Decompress, validate and decode the status list token JWT
    /// 4. Retrieve the status at the specified index from the `StatusList`
    ///
    /// Returns `FailedToGetCredentialStatus` for operational issues (can't fetch, decode, etc.)
    /// Returns `CredentialStatusInvalid` for actual status validation failures.
    async fn verify_status_claim(&self, status_claim: serde_json::Value) -> Result<(), CredentialStatusCheckerError> {
        if let Ok(status_claim) = serde_json::from_value::<StatusClaim>(status_claim) {
            info!("Succesfully parsed `status` claim {status_claim:#?}");

            // 3xx redirects should be followed, but infinite loops are caught after 5 redirects.
            // The timeout of 10 seconds is an estimated guess of how long a status list request should take at maximum.
            let client = Client::builder()
                .redirect(Policy::limited(5))
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // The `accept_header` parameter determines the expected response format, currently we can only process the JWT format.
            let response = client
                .get(status_claim.status_list.uri)
                .header(header::ACCEPT, StatusListTokenResponseType::Jwt.to_string())
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // TODO: Move this logic to OAuth TSL library
            let is_gzipped = response
                .headers()
                .get(header::CONTENT_ENCODING)
                .is_some_and(|encoding| encoding == "gzip");

            let bytes = response
                .bytes()
                .await
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // Check if the response is gzip encoded and decompress if necessary.
            let status_list_jwt = if is_gzipped {
                decompress_gzip(&bytes)
                    .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))
            } else {
                String::from_utf8(bytes.to_vec())
                    .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))
            }?;

            let jwt_header = decode_header(&status_list_jwt)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;
            let kid = jwt_header
                .kid
                .ok_or(CredentialStatusCheckerError::FailedToGetCredentialStatus(
                    "No KID found".to_string(),
                ))?;

            // TODO: Resolving the public key stays in ssi-agent, perhaps via a trait from the OAuth TSL library
            let public_key_jwk = self
                .verification_material_resolver
                .resolve_public_key(&kid)
                .await
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // Convert the `IotaIdentityJwk` first into a `JsonWebTokenJwk` and then into a `DecodingKey`.
            let decoding_key = public_key_jwk
                .to_json()
                .ok()
                .and_then(|public_key| JsonWebTokenJwk::from_json(&public_key).ok())
                .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
                .ok_or(CredentialStatusCheckerError::FailedToGetCredentialStatus(
                    "Failed to create decoding key".to_string(),
                ))?;

            // TODO: move this logic to the OAuth TSL library.
            let decoded_jwt = decrypt_status_list_token(&status_list_jwt, decoding_key)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            let status_list: StatusList = decoded_jwt.claims.encoded_status_list.try_into().map_err(|_| {
                CredentialStatusCheckerError::FailedToGetCredentialStatus("Failed to decode status list".to_string())
            })?;

            // Converting it into StatusType means we choose the default status types as defined in the OAuth TSL library.
            // However, get_status returns a u8, which can be interpreted anyway ssi-agent wants as explained in the OAuth TSL spec, meaning we could define our own status types here.
            let status = StatusType::try_from(status_list.get_status(status_claim.status_list.idx as usize).map_err(
                |_| {
                    CredentialStatusCheckerError::FailedToGetCredentialStatus(
                        "Failed to get credential status from index".to_string(),
                    )
                },
            )?)
            .map_err(|_| {
                CredentialStatusCheckerError::FailedToGetCredentialStatus(
                    "Failed to get credential status from index".to_string(),
                )
            })?;

            match status {
                StatusType::VALID => Ok(()),
                _ => Err(CredentialStatusCheckerError::CredentialStatusInvalid),
            }
        } else {
            warn!("Unable to parse `status` claim as per the OAuth Token Status List specification, skipping credential status check");
            Ok(())
        }
    }
}

// TODO: move this structs to the OAuth TSL library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusClaim {
    pub status_list: StatusListClaim,
}

// TODO: move this structs to the OAuth TSL library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusListClaim {
    pub idx: u64,
    pub uri: Url,
}

#[cfg(test)]
mod tests {
    use super::*;

    use agent_secret_manager::subject::Subject;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn test_check_jwt_status_claim() {
        // A credential with no "status" claim should be considered valid.
        let subject = Subject::test_subject().await;
        let checker = CredentialStatusChecker {
            verification_material_resolver: Arc::new(subject),
        };

        let result = checker.check_credential_status(serde_json::json!({})).await;

        assert!(result.is_ok(), "Empty status claim should be gracefully skipped");

        // A credential with a "status" claim that has an unrecognized status type should be considered valid (since we don't know how to check the status, we shouldn't fail the validation just because of that).
        let result = checker
            .check_credential_status(serde_json::json!({
                "id": "https://example.com/status/123",
                "type": "UnrecognizedStatusType"
            }))
            .await;

        assert!(result.is_ok(), "Unrecognized status type should be gracefully skipped");

        // Create a new mock server and retreive it's url.
        let mock_server = MockServer::start().await;
        let server_url = mock_server.uri();

        let response_bytes: &[u8] = b"\x1f\x8b\x08\0\0\0\0\0\x02\xff-\x95\xdb\x92\xa3:\x12E\xff\xe8\x84$\x8cO\xf9\xb10\x88K\x19\xd1\x08]@/\x13\x80\xa8\xc6H`\xaaL\x97m\xbe~\xd4\x13\xf3\x9e\x11\x99{e\xee\x9d\xc3+\x03C\xfd~-\xae\xd9\xaec\x0ctM\xeemM\xc0\x07Z=}N\xef\xe9\x8c\xef\xea\x95\x1eS#\x0c\xc5$H\xaf\x8fk\x8b\xacI\xa7\xdbU\xc5\xd6\x14\xf3f\x07\xb6\x1e\x89\x81\xdf\n\xf0\x03\x89\xfc\x90\xd5\x01Q\x1c\x1f\n\xb9^\nIf\"z@\xf7qi\xe0\xcd\x1f\x12\xe1U\x8bJr\xc9=\r\xfc\xef\"\xdaB\xe5\x95\xd30)\xd2\"\x8d\x8b\x90\x16%\x80\x0f&E0\x84\xf6\xab\xda\xed\xa4&Q\x93\xa8?(D\xda\xa2\x1e\xa1\x06+\xa2;\xb6\xc4\xd3E\xbb\x8f\x97\x12i\x90.\xe0\x9f\xe1\xe54\xc8\xf4\xaf\x96\x9bN\xe8\xa3\xdfo?\x17\x04\xfd\x8b\xa4?\x9d\xc4kw\xf5\xed\x10\xe3\xad\x8f\x9f\xf62\x9f^\xeauZUM\xe7\x8b<\x8d\xba\xa6\xb7KM\x7fZ$\xfe\\\x9c\xfe\xa6\xa6\xb0\x7f\xc1\xff\xb1\xb8\xec\xef\xd7\xcb9[\x9b\xba\xbc\x16S\xe4\x91\xbd|\xe4!\xdf\t{\xbf\xa7\xcb\xffk\xe1\xe9\x7f\xb5\xe9\xb4\xfe\x9b\xce\xd9\xaa\x93\xdc\xd5\xfe\xe5\xf7\xdc\x1d\xc7c:\x8b\xa2CT\x8aZ\\JG\xb2\x12T\xd0\x90\x84J\xfe\xf6\xabZ\xdc\x94\xc0\xb6\x10\xa7\xbb\x0c\xb5d\x9c\x969V\x17\x81V\xd4\x87\xe3\xa4\x16,y\xa4\xb2\x9c\xbf\xc1\x1ab\"\xe3\xf4\xc5\xcf\x90vK\x16\x95W\xf8\xd1\xd7\xc4\x08\xbb\xbe\xba$\xf3/\x82C\xb6G\xa8\x91\xd4\xed\xc0/J+r-}$\x96\xfc\xa0\xc2\xe6\xa0\xa2\x065sv \xe0\x80dd\x9e\x14\xe1\xef\xa6^=\xe6\t5Hr\x90\x91:\x900h\x98\x1d\xef\xa4VP\xc8\xc6kv\xbcq+\xe6!\xda\x0e\x95\xa5M\x9f\xe0\x0f\xb2\x93s\r\x9e\xdf\xcd\x94\xbdr\xa6/\xcdTn|\xcf\xeed\xb7\xbf\xb4U\xcfKm#5\xe9\x83HJOG[\x9e/\xc1\xd6E\xdaS\xb5\xf1\xa5\xa1w2\xa9\xa3\x10\xd9wW\xa7\x1e\x89q\xc6\x17\xf2GM\xa3\xe8f\x1fh~BZ\x04\xa5\x9e)\x11\xbb\xfa\xd3\xd7Y\xc3B\x12)\x86\xe7.:\x15\xad\x17\xa4\x9aE\x0fjV\xe8\xb0A\xa7\xefJ&\x9bk\x1cP\xc2\x04/\x92,\xe8\x05\xbe\x08<\x9a\xca\xcd5\x98\xb1\xe8&E\x19\xba\x83\x06\xc0\xa3\x02\xa7o6\xd1\x94r<\x97\xfc\xf4\xd3\xc0,Rp\xcct2\xbe8\x87G\xcd\xca\x17G4\xa0q\xf6P2z\x12\x9e}\x14\xf2\xf1h\xac\x90\xc4<a\x07\xc5F&j;\xa9\x7f\x94yn<Q\xb6\x82\xe2U\x03q\x96\x89\xd3c\x1e\x8f\x9e\x8dM\xcfl\xd6N\xcar,\x90\x12A\\\xc2\x14\x95/\xb0\xf7\xf3\x88\xe5\x82\xe1\xa5\xd6\x1f\r\xf73!\xc5\xf1/\x07\x16\xd3_\xca\x92\xa2Z\xde\x11\r\xf1\xaa\xed\xfa\xc9\0-\xfa\x19>]=\xa8$v\x0c\x01\xe8\xa1])OQ>\xe9e\x08\xb3@%$\xa4R?(\x87_\xd5$~x\xa2\xd3V\xe8\xb1\x0f\xf9\xa6\xf9\xdb\xabZ\xb2]\xf1\xd3E\xec\xef>\x81\xe3\xdc9FR\x9e\xe8\xc0\xfd\xdd\xb1\x0e*\x96\x91j\xca\xc2Rf\x9e\x8e\xe9C%\xc1Q@\xb3q\xb9\xfe\x94\xd3\xfbc\x88\xc8L\x93\xf5\x17\xe1\xfcA\xe6\xdf\x87\x82\x05\x8a\x1a\xbb\x95\xc87\x17yx\xc9\x19\x93\xce\xa4{/\xb7\xa5\x88\x9f\x13\rs$<\x9a\xe7H\xd7\xfd\x9c\x95Th\\\xf1mS\xbbH\x8aP\0j0\xd1\xc0\xe2\x16\xe0\xb4t\xde!2:\xd4\xc8\x9e\xd9b\x8f\x95\xbc\xed5\xd4\x0b\r\x85\xcf\xc2@\\\xa4.\xf3\xf9D\x18\xeba%\xf0\xab\x99\"T\x1a\xdf6\x0b\xe6rV\x87\x1e\xdd\x80Jp #K\x18\x16\xce3p/jqlv[U\xb3 }\xa4\xbf:\x9e>)\xa7U/\x82_\xf2\n\0\x9b\xf9\xc6\xa2\xad\xe9\x80P\x82\xe3\xef\xb2\x1e\x05]V\xa7M[\x01\xd3\xbd\xbbBY\"\x95\xf7;\xae\x1d\x03\x9f\01\x95\xc6\x9a.~6%\xb3?\x1d K\xc3iX\xf2\xdf\xd0y\x9fU\x1c_Z\x83\x8f\xc4\n\xdcM\xf4\x99/+\x1aB\x03\x1c\x97\\'+\x1fB\xfdQ\x19E\xaaY\x1f\x89G\x976$\x89\xcb\x80\x9b\x9be\x1bf\xff\xd2O\xeen*\xb0k\x04|\n\x03\xeb\xb22\xcaa@\xfbY\x04\xc4y\xa5M\xde\x91\xdb\x9b\x10P\xb72\x1e\x0b\xce\x9b\x03M\x82q0\xa7\xb3\x9a\xd5Vy\xfa(\x10\xae)\xd8>\x87\xd8\xb7\x04\xe2\xf2\xaf\xb7y,\xe2\x16\xebX\xef8 \xc9\xedP\xce\xd9\xdd\xddy\xd4\xcc\x9b\xd7\x874)\xe4\x18\xb7\xf1\xcdc@|\xeaE0-\xb1i\x99\xd88(\x1f\"\xb1\x8aF\xdb(8I\x99\xcd\xd1\xc0\xb3\x80Z\xfc\xc3\xec\xc9q\x1bww\x97\x9c\x18\xbf%&\xfb\xc9\xcf\xc0\x1b\xe2\x07\xc8c{\x18\x9c\xa7\x04\x0eL\x7f\x86\x1f\x1d\xd0\x8cE\xa3)\xc4\xfag\x98\xd5\x1f\x973\x86\xe3\xc6\xa3\xa0D%<}\xf6\x9e\x95\"\x14_,\xe1\x1eA$&Q\xfej\xf8\xb85\xfbxo\x19\xf7\xdb%\xcbu\x88I\xbb\x08\xbf\xac\xb3o\x1e\xdf\xa1\xcb\xa7\xb0Jp\xda\x9a\xa77\x18\x9c\xf4 \xdb\xb9\xc0~_\xeb\xe6\xc2F\xdc\xc8\xe7W\x89\xb0a\xb3\xbe\x88x]k\xa0\x1f<\x1cK\xb9\x10\xdb\t\xf2\xe3\xbc\xf05\xe0\x14\x92\xd0\xe6-\x80\xa0\x04\xe9#\x17\xa7\xb8\x14t\xa7\x90\xe6\x033\x0f\x86V\xa8\xac\xf6r\x90{z\x0f\x84\xb0\xd9\x9e'z\xab\xb8\xce\x14\xf2o\x04\xe0\xb6J\xecWcm\xe3<\x9f\x14x]J\x9e\xc5<\xcaV)u\xe1r\xc5\x946\x83%O\x1f\xad\xdb\x05\x93\xe3C\xbb\xbd\xb6a\xe3\x94SZ\x1aw\xa78\xf0s\xa6\x82\x8e\xb9\xdb\xad\xf55\x17\xc1q\xe0\xa2-m\x90+\x94\xfaN\xd3MD\xe4C\xce\xeb2H<\x93\xf8y\xed8<_\xe4xv\x8c?UH}io\x9b\xcb;\x94\xcb\x15:\xdf8\x9f\xda\xad\xe0\"\x92\xb3\x81\xe5\x14$\x94\xf5[\x85 n\xe71\xceA\xba\xd3\x98\xde\xbaI\xdb\x0e\xad1g\x99Q\xc0\xed%T[\xc7\x85\xcbB\xff\xbb\x9d\xfdE\xcf\xbf}\x8aFP!E\xfb\xbf\xbd\x049W\x8bM\x1c\x84\x94\"\xbdR1:\x1b\xbe\xf9|o\x1e<i\x1e\x82\x8b\xa41\xd9\xa1dY\xcc\x0cxu\xf3)uY\xe95\xf3\xf8\xf7\xe7^Y\xed\xbe4z{\xba\x1eY\x0b\x83\xa9A\xa7\x8c2\xe1\xb4f_zv\x1e\x83\x81i\xe3\xf5X\x85\"h\xe4jss\xca]Vp>\x95\x87\xde\x8e\x9f\x03\x16\xb42w\xd4\xb8~\x15\xd0Mc\xc6\xbb\xfb(\x98 ;\xf2\xc4e\x16\xb7\x17\x05\xed\xac\xf0mg&K\x1bAJ\r\xe0\xaf\xd2\x13\xb2\x8d\\Fs\xd1p\x97?\xa2\xfe\xfdP\xc0\xba~X\xa8x\xc5\xad\xbb\x89\xf6\x0c6\x8d\xb6s\x0fDI\xed\xfbS\\a\xdc\xc5*n\xbd\x0c6\xb6\xd9\xbae\xfd& Gj>]9\xbc=\xe9|R]\x98\xc3\xca<\xeb\xd2\x8e\xab0\xf0\xd6\xcd\xe2RY\xfb\x9d\xcb7\xe7w\xfa\xd9\xc7\x98\xb9\x19\xa9Jh3\x18\xe7udW\x1d\xe6\x07\x812\xd9\xa3\xfe\xfaY\x83\x7f\xeaq\x1c\x0e(4\x9f\xe5\xb5\x8b;\xf0o\xfa{\xc4\xf7a\xb9\x8d\xe4\xb4\xbe=?\xab\xdbCdd\xfex\xbby\xe97O\xb8?Z\xcf/o\xb7\x97\xf4\xba\xd1KL`\xcc\x9f\xff\x08k\xda\xf9\xf5\x9f\xde{\xfd\xf4\xdf\xd3{\xf6\xfe\xfb\xbfl\xc1\xd9\xa7\xca\t\0\0";
        Mock::given(method("GET"))
            .and(path("/status_list"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Encoding", "gzip")
                    .set_body_raw(response_bytes, "application/statuslist+jwt"),
            )
            .mount(&mock_server)
            .await;

        // The index 123 is set to INVALID in the given mock response status list.
        let result = checker
            .check_credential_status(serde_json::json!({
                "status_list": {
                    "idx": 123,
                    "uri": format!("{}/status_list", server_url),
                }
            }))
            .await;

        assert!(result.is_err(), "Invalid credential status should fail");

        // The index 2 is set to VALID in the given mock response status list.
        let result = checker
            .check_credential_status(serde_json::json!({
                "status_list": {
                    "idx": 2,
                    "uri": format!("{}/status_list", server_url),
                }
            }))
            .await;

        assert!(result.is_ok(), "Valid credential status should pass");
    }
}
