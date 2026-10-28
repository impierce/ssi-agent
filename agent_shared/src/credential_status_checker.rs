use anyhow::bail;
use async_trait::async_trait;
use flate2::read::GzDecoder;
use identity_kit::{
    did::DidUrl,
    resolver::{KeyReference, KeyResolver, KeyRole},
    status_list::{Status, StatusListToken},
    vc::validation::{CredentialStatus, StatusCheck, StatusError},
};
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use reqwest::{header, redirect::Policy, Client};
use std::{io::Read, sync::Arc, time::Duration};
use thiserror::Error;
use tracing::{info, warn};

/// The largest status list token, compressed or not, the checker reads. The status list URI comes from the credential,
/// so its server decides how much it sends. This is the body limit of IdentityKit's `ReqwestClient`.
const MAX_STATUS_LIST_TOKEN_SIZE: usize = 1 << 20;

/// The clock skew tolerated between the status list issuer and this verifier for `iat` and `exp`, the default leeway of
/// the `jsonwebtoken` crate.
const CLOCK_SKEW_LEEWAY: chrono::TimeDelta = chrono::TimeDelta::seconds(60);

#[derive(Error, Debug)]
pub enum CredentialStatusCheckerError {
    #[error("Failed to get credential status: {0}")]
    FailedToGetCredentialStatus(String),
    #[error("Credential status is invalid")]
    CredentialStatusInvalid,
}

pub struct CredentialStatusChecker {
    /// Resolves the key that signed a status list token.
    pub key_resolver: Arc<dyn KeyResolver>,
}

impl CredentialStatusChecker {
    // TODO: in the future it would be nice to have a flag every time it's called or only in the config to choose between strict validation also failing on operational errors or lenient validation as is currently.
    /// This function is basically a wrapper around `verify_status_claim` which allows us to differentiate between operational errors (unable to fetch or decode the status list token) and actual validation failures (credential status is invalid).
    pub async fn check_credential_status(
        &self,
        status_claim: serde_json::Value,
    ) -> Result<(), CredentialStatusCheckerError> {
        match self.verify_status_claim(status_claim).await {
            Ok(_) => Ok(()),
            // Unable to verify status is not a validation failure, so it's logged as a warning and the validation is skipped.
            Err(CredentialStatusCheckerError::FailedToGetCredentialStatus(e)) => {
                warn!("Unable to verify credential status, skipping status check: {}", e);
                Ok(())
            }
            // Actual validation failures propagate as errors
            Err(e) => Err(e),
        }
    }

    /// Internal helper which does the following steps, as per the OAuth Token Status List specification:
    /// 1. Tries to parse the `status` claim from the credential
    /// 2. Fetch the status list token JWT
    /// 3. Decompress, validate and decode the status list token JWT
    /// 4. Retrieve the status at the specified index from the `StatusList`
    ///
    /// Returns `FailedToGetCredentialStatus` for operational issues (can't fetch, decode, etc.)
    /// Returns `CredentialStatusInvalid` for actual status validation failures.
    async fn verify_status_claim(&self, status_claim: serde_json::Value) -> Result<(), CredentialStatusCheckerError> {
        if let Some(status_list_ref) = serde_json::from_value::<Status>(status_claim)
            .ok()
            .and_then(|status| status.status_list)
        {
            info!("Succesfully parsed `status` claim {status_list_ref:#?}");

            // 3xx redirects should be followed, but infinite loops are caught after 5 redirects.
            // The timeout of 10 seconds is an estimated guess of how long a status list request should take at maximum.
            let client = Client::builder()
                .redirect(Policy::limited(5))
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // The `accept_header` parameter determines the expected response format, currently we can only process the JWT format.
            let mut response = client
                .get(&status_list_ref.uri)
                .header(header::ACCEPT, StatusListToken::MEDIA_TYPE)
                .header(header::ACCEPT_ENCODING, "gzip")
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // TODO: Move this logic to IdentityKit
            let is_gzipped = response
                .headers()
                .get(header::CONTENT_ENCODING)
                .is_some_and(|encoding| encoding == "gzip");

            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?
            {
                if bytes.len() + chunk.len() > MAX_STATUS_LIST_TOKEN_SIZE {
                    return Err(CredentialStatusCheckerError::FailedToGetCredentialStatus(format!(
                        "The status list token is larger than {MAX_STATUS_LIST_TOKEN_SIZE} bytes"
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }

            // Check if the response is gzip encoded and decompress if necessary.
            let status_list_jwt = if is_gzipped {
                decompress_gzip(&bytes)
                    .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))
            } else {
                String::from_utf8(bytes)
                    .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))
            }?;

            let jwt_header = decode_header(&status_list_jwt)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;
            let kid = jwt_header
                .kid
                .ok_or(CredentialStatusCheckerError::FailedToGetCredentialStatus(
                    "No KID found".to_string(),
                ))?;

            let kid = DidUrl::parse(&kid)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;
            let public_key_jwk = self
                .key_resolver
                .resolve_key(&KeyReference::DidUrl(kid), KeyRole::Issuer)
                .await
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            let decoding_key = public_key_jwk
                .to_decoding_key()
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // TODO: move this logic to IdentityKit.
            let status_list_token = decode_status_list_token(&status_list_jwt, &decoding_key)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            // Checks that `sub` is the referenced URI, that the token hasn't expired and that the index is in range.
            let status = status_list_token
                .status_of(&status_list_ref, chrono::Utc::now() - CLOCK_SKEW_LEEWAY)
                .map_err(|e| CredentialStatusCheckerError::FailedToGetCredentialStatus(e.to_string()))?;

            if status.is_valid() {
                Ok(())
            } else {
                Err(CredentialStatusCheckerError::CredentialStatusInvalid)
            }
        } else {
            warn!("Unable to parse `status` claim as per the OAuth Token Status List specification, skipping credential status check");
            Ok(())
        }
    }
}

/// Checks the `status` claims of the credentials in a presentation, with the same lenient handling as
/// [`CredentialStatusChecker::check_credential_status`].
#[async_trait]
impl StatusCheck for CredentialStatusChecker {
    async fn check(
        &self,
        status: CredentialStatus<'_>,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StatusError> {
        let CredentialStatus::StatusClaim(status) = status else {
            return Err(StatusError::Unsupported(status.mechanism()));
        };
        let status_claim = serde_json::to_value(status).map_err(|e| StatusError::Invalid(e.to_string()))?;

        self.check_credential_status(status_claim)
            .await
            .map_err(|e| StatusError::Invalid(e.to_string()))
    }
}

/// Decompresses a gzip-encoded response body of at most [`MAX_STATUS_LIST_TOKEN_SIZE`] bytes once decompressed.
pub fn decompress_gzip(data: &[u8]) -> std::io::Result<String> {
    let mut decompressed = String::new();
    GzDecoder::new(data)
        .take(MAX_STATUS_LIST_TOKEN_SIZE as u64 + 1)
        .read_to_string(&mut decompressed)?;
    if decompressed.len() > MAX_STATUS_LIST_TOKEN_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("The decompressed status list token is larger than {MAX_STATUS_LIST_TOKEN_SIZE} bytes"),
        ));
    }
    Ok(decompressed)
}

/// Verifies the signature of a status list token JWT and decodes its claims. Tokens issued in the future are rejected;
/// `sub` and `exp` are checked by [`StatusListToken::status_of`].
pub fn decode_status_list_token(status_list_jwt: &str, decoding_key: &DecodingKey) -> anyhow::Result<StatusListToken> {
    let header = decode_header(status_list_jwt)?;
    if header.typ.as_deref() != Some(StatusListToken::TYP) {
        bail!("Invalid `typ` in the status list token header: {:?}", header.typ);
    }

    let mut validation = Validation::new(header.alg);
    validation.set_required_spec_claims(&["sub"]);
    validation.validate_exp = false;
    validation.validate_aud = false;

    let status_list_token = decode::<StatusListToken>(status_list_jwt, decoding_key, &validation)?.claims;

    if status_list_token.iat > (chrono::Utc::now() + CLOCK_SKEW_LEEWAY).timestamp() {
        bail!(
            "The status list token is issued in the future: `iat` is {}",
            status_list_token.iat
        );
    }

    Ok(status_list_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    use flate2::{write::GzEncoder, Compression};
    use identity_kit::{
        methods::key::DidKey,
        resolver::Resolver,
        status_list::{StatusList, StatusType},
    };
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use std::io::Write;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    /// The `did:key` of the Ed25519 key of RFC 8032, section 7.1, test 1.
    const TEST_DID: &str = "did:key:z6MktwupdmLXVVqTzCw4i46r4uGyosGXRnR3XjN4Zq7oMMsw";

    /// The PKCS #8 encoding of that key's secret.
    const TEST_PKCS8: [u8; 48] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20, 0x9d, 0x61,
        0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69,
        0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
    ];

    /// That key's public key.
    const TEST_PUBLIC_KEY: [u8; 32] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a, 0x0e, 0xe1,
        0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
    ];

    /// The index set to `INVALID` in the status list of [`status_list_token`]. Every other index is `VALID`.
    const INVALID_INDEX: u64 = 123;

    fn checker() -> CredentialStatusChecker {
        CredentialStatusChecker {
            key_resolver: Arc::new(Resolver::builder().with(DidKey::new()).build()),
        }
    }

    fn status_list_token(sub: &str, iat: chrono::DateTime<chrono::Utc>) -> StatusListToken {
        let mut status_list = StatusList::new(2, 1024).unwrap();
        status_list.set(INVALID_INDEX, StatusType::INVALID).unwrap();
        StatusListToken::new(sub, iat, status_list)
    }

    fn sign(status_list_token: &StatusListToken) -> String {
        let header = Header {
            typ: Some(StatusListToken::TYP.to_string()),
            kid: Some(format!("{TEST_DID}#{}", TEST_DID.trim_start_matches("did:key:"))),
            ..Header::new(Algorithm::EdDSA)
        };
        encode(&header, status_list_token, &EncodingKey::from_ed_der(&TEST_PKCS8)).unwrap()
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// Serves `body` at `/status_list`, gzip-encoded if `gzipped`. Returns the server and the URI.
    async fn serve(body: Vec<u8>, gzipped: bool) -> (MockServer, String) {
        let mock_server = MockServer::start().await;
        let mut response = ResponseTemplate::new(200).set_body_raw(body, StatusListToken::MEDIA_TYPE);
        if gzipped {
            response = response.insert_header("Content-Encoding", "gzip");
        }
        Mock::given(method("GET"))
            .and(path("/status_list"))
            .respond_with(response)
            .mount(&mock_server)
            .await;
        let uri = format!("{}/status_list", mock_server.uri());
        (mock_server, uri)
    }

    fn status_claim(uri: &str, idx: u64) -> serde_json::Value {
        serde_json::json!({
            "status_list": {
                "idx": idx,
                "uri": uri,
            }
        })
    }

    #[tokio::test]
    async fn test_check_jwt_status_claim() {
        let checker = checker();

        // A credential with no "status" claim should be considered valid.
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

        let mock_server = MockServer::start().await;
        let uri = format!("{}/status_list", mock_server.uri());
        let jwt = sign(&status_list_token(&uri, chrono::Utc::now()));
        Mock::given(method("GET"))
            .and(path("/status_list"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Encoding", "gzip")
                    .set_body_raw(gzip(jwt.as_bytes()), StatusListToken::MEDIA_TYPE),
            )
            .mount(&mock_server)
            .await;

        let result = checker.check_credential_status(status_claim(&uri, INVALID_INDEX)).await;

        assert!(result.is_err(), "Invalid credential status should fail");

        let result = checker.check_credential_status(status_claim(&uri, 2)).await;

        assert!(result.is_ok(), "Valid credential status should pass");
    }

    #[tokio::test]
    async fn test_status_list_token_for_another_uri_is_not_used() {
        let jwt = sign(&status_list_token(
            "https://example.org/other_status_list",
            chrono::Utc::now(),
        ));
        let (_mock_server, uri) = serve(gzip(jwt.as_bytes()), true).await;

        let result = checker().verify_status_claim(status_claim(&uri, INVALID_INDEX)).await;

        assert!(
            matches!(
                result,
                Err(CredentialStatusCheckerError::FailedToGetCredentialStatus(_))
            ),
            "A status list token whose `sub` isn't the referenced URI must not be used, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_status_list_token_larger_than_the_limit_is_not_read() {
        let (_mock_server, uri) = serve(vec![b'a'; MAX_STATUS_LIST_TOKEN_SIZE + 1], false).await;

        let result = checker().verify_status_claim(status_claim(&uri, INVALID_INDEX)).await;

        assert!(
            matches!(&result, Err(CredentialStatusCheckerError::FailedToGetCredentialStatus(e)) if e.contains("larger than")),
            "{result:?}"
        );
    }

    #[test]
    fn test_decompress_gzip_limits_the_decompressed_size() {
        let at_limit = vec![b'a'; MAX_STATUS_LIST_TOKEN_SIZE];
        assert_eq!(
            decompress_gzip(&gzip(&at_limit)).unwrap().len(),
            MAX_STATUS_LIST_TOKEN_SIZE
        );

        let over_limit = vec![b'a'; MAX_STATUS_LIST_TOKEN_SIZE + 1];
        assert!(decompress_gzip(&gzip(&over_limit)).is_err());
    }

    #[test]
    fn test_decode_status_list_token_tolerates_clock_skew_for_iat() {
        let decoding_key = DecodingKey::from_ed_der(&TEST_PUBLIC_KEY);
        let now = chrono::Utc::now();

        let jwt = sign(&status_list_token(
            "https://example.org/status_list",
            now + chrono::TimeDelta::seconds(30),
        ));
        assert!(decode_status_list_token(&jwt, &decoding_key).is_ok());

        let jwt = sign(&status_list_token(
            "https://example.org/status_list",
            now + chrono::TimeDelta::seconds(120),
        ));
        assert!(decode_status_list_token(&jwt, &decoding_key).is_err());
    }

    #[tokio::test]
    async fn test_status_list_token_tolerates_clock_skew_for_exp() {
        let now = chrono::Utc::now();
        let mock_server = MockServer::start().await;
        let uri = format!("{}/status_list", mock_server.uri());

        let mut just_expired = status_list_token(&uri, now - chrono::TimeDelta::seconds(600));
        just_expired.exp = Some((now - chrono::TimeDelta::seconds(30)).timestamp());
        Mock::given(method("GET"))
            .and(path("/status_list"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sign(&just_expired), StatusListToken::MEDIA_TYPE))
            .mount(&mock_server)
            .await;

        let result = checker().verify_status_claim(status_claim(&uri, INVALID_INDEX)).await;

        assert!(
            matches!(result, Err(CredentialStatusCheckerError::CredentialStatusInvalid)),
            "A token that expired within the leeway should be used, got {result:?}"
        );

        let mut expired = just_expired;
        expired.exp = Some((now - chrono::TimeDelta::seconds(120)).timestamp());
        mock_server.reset().await;
        Mock::given(method("GET"))
            .and(path("/status_list"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sign(&expired), StatusListToken::MEDIA_TYPE))
            .mount(&mock_server)
            .await;

        let result = checker().verify_status_claim(status_claim(&uri, INVALID_INDEX)).await;

        assert!(
            matches!(
                result,
                Err(CredentialStatusCheckerError::FailedToGetCredentialStatus(_))
            ),
            "An expired token should not be used, got {result:?}"
        );
    }
}
