//! Signing in to STACKIT as a service account, for an access token that STACKIT accepts for about 15 minutes.
//!
//! `UniCore` presents the workload identity token its host gives it (ADR 0011). A service account key file is only for
//! local development: it is a long-lived secret, and nothing in `UniCore` reads one.

use crate::answer::refusal_message;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::debug;
use url::Url;
use uuid::Uuid;

/// Where STACKIT exchanges a workload identity token for an access token.
const WORKLOAD_IDENTITY_TOKEN_URL: &str = "https://accounts.stackit.cloud/oauth/v2/token";
/// How long before it expires an access token is replaced, so that no request is sent with one about to expire.
const RENEWAL_MARGIN: Duration = Duration::from_mins(1);
/// How long the assertion signed with a key file is valid.
const ASSERTION_LIFETIME: Duration = Duration::from_hours(1);

/// How `UniCore` signs in to STACKIT as a service account.
#[derive(Debug)]
pub enum SignIn {
    /// Workload identity federation: the token in `token_file`, which the host renews, is exchanged at `token_url`
    /// for an access token of `service_account_email`. The file is read again for every sign-in.
    WorkloadIdentity {
        token_file: PathBuf,
        service_account_email: String,
        token_url: Url,
    },
    /// A service account key file, for local development only.
    KeyFile(ServiceAccountKey),
}

impl SignIn {
    /// Workload identity federation at STACKIT's token endpoint, with the token in `token_file`.
    #[must_use]
    #[allow(clippy::missing_panics_doc, reason = "the token URL is a constant")]
    pub fn workload_identity(token_file: PathBuf, service_account_email: String) -> Self {
        Self::WorkloadIdentity {
            token_file,
            service_account_email,
            token_url: Url::parse(WORKLOAD_IDENTITY_TOKEN_URL).expect("a valid URL"),
        }
    }

    /// Signs in with the service account key file `json`, as STACKIT's portal hands it out. For local development only.
    ///
    /// # Errors
    ///
    /// When `json` is not a service account key file with a private key.
    pub fn key_file(json: &str) -> Result<Self, serde_json::Error> {
        Ok(Self::KeyFile(serde_json::from_str(json)?))
    }

    /// Exchanges this sign-in for a new access token.
    async fn exchange(&self, http: &reqwest::Client) -> Result<TokenResponse, SignInError> {
        let request = match self {
            Self::WorkloadIdentity {
                token_file,
                service_account_email,
                token_url,
            } => {
                let token =
                    tokio::fs::read_to_string(token_file)
                        .await
                        .map_err(|source| SignInError::MissingToken {
                            path: token_file.clone(),
                            source,
                        })?;
                http.post(token_url.clone()).form(&[
                    ("grant_type", "client_credentials"),
                    (
                        "client_assertion_type",
                        "urn:schwarz:params:oauth:client-assertion-type:workload-jwt",
                    ),
                    ("client_assertion", token.trim()),
                    ("client_id", service_account_email),
                ])
            }
            Self::KeyFile(key) => http.post(key.credentials.token_endpoint.clone()).form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &key.assertion()?),
            ]),
        };

        let response = request.send().await.map_err(SignInError::Unreachable)?;
        let status = response.status();
        let body = response.text().await.map_err(SignInError::Unreachable)?;
        if !status.is_success() {
            return Err(SignInError::Refused {
                status: status.as_u16(),
                message: refusal_message(&body),
            });
        }
        serde_json::from_str(&body).map_err(|error| SignInError::Unexpected(error.to_string()))
    }
}

/// A STACKIT service account key file. Its private key is never shown, also not by `Debug`.
#[derive(Deserialize)]
pub struct ServiceAccountKey {
    credentials: KeyFileCredentials,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyFileCredentials {
    kid: String,
    iss: String,
    sub: String,
    aud: String,
    private_key: String,
    token_endpoint: Url,
}

#[derive(Serialize)]
struct AssertionClaims<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: &'a str,
    jti: String,
    iat: u64,
    exp: u64,
}

impl ServiceAccountKey {
    /// A JWT signed with the private key that asserts to be the key file's service account.
    fn assertion(&self) -> Result<String, SignInError> {
        let credentials = &self.credentials;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let header = Header {
            kid: Some(credentials.kid.clone()),
            ..Header::new(Algorithm::RS512)
        };
        let claims = AssertionClaims {
            iss: &credentials.iss,
            sub: &credentials.sub,
            aud: &credentials.aud,
            jti: Uuid::new_v4().to_string(),
            iat: now,
            exp: now + ASSERTION_LIFETIME.as_secs(),
        };
        let key = EncodingKey::from_rsa_pem(credentials.private_key.as_bytes()).map_err(SignInError::KeyFile)?;
        jsonwebtoken::encode(&header, &claims, &key).map_err(SignInError::KeyFile)
    }
}

impl fmt::Debug for ServiceAccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let credentials = &self.credentials;
        f.debug_struct("ServiceAccountKey")
            .field("kid", &credentials.kid)
            .field("sub", &credentials.sub)
            .field("token_endpoint", &credentials.token_endpoint.as_str())
            .finish_non_exhaustive()
    }
}

/// Why `UniCore` could not sign in to STACKIT.
#[derive(Debug, Error)]
pub enum SignInError {
    /// The host gave no workload identity token, or it can't be read.
    #[error("The workload identity token can't be read from `{}`: {source}", path.display())]
    MissingToken { path: PathBuf, source: std::io::Error },
    /// STACKIT refused the token or the key file, such as when no federated identity provider trusts the token.
    #[error("STACKIT refused the sign-in ({status}): {message}")]
    Refused { status: u16, message: String },
    /// The sign-in got no answer, such as for a network error or a timeout.
    #[error("STACKIT's sign-in could not be reached: {0}")]
    Unreachable(#[source] reqwest::Error),
    /// STACKIT accepted the sign-in, but its answer isn't an access token.
    #[error("STACKIT's sign-in answered with no access token: {0}")]
    Unexpected(String),
    /// The key file's private key can't sign the assertion.
    #[error("The service account key file can't sign in: {0}")]
    KeyFile(#[source] jsonwebtoken::errors::Error),
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    /// Seconds.
    expires_in: u64,
}

/// An access token, kept until it is about to expire.
struct AccessToken {
    secret: String,
    renew_at: Instant,
}

/// The access token of one sign-in, signed in for again once it is about to expire or STACKIT refuses it. Concurrent
/// requests wait for one sign-in rather than each signing in.
pub(crate) struct AccessTokens {
    sign_in: SignIn,
    http: reqwest::Client,
    current: Mutex<Option<AccessToken>>,
}

impl AccessTokens {
    pub(crate) fn new(sign_in: SignIn, http: reqwest::Client) -> Self {
        Self {
            sign_in,
            http,
            current: Mutex::new(None),
        }
    }

    /// An access token that doesn't expire within the renewal margin.
    pub(crate) async fn get(&self) -> Result<String, SignInError> {
        let mut current = self.current.lock().await;
        if let Some(token) = current.as_ref().filter(|token| Instant::now() < token.renew_at) {
            return Ok(token.secret.clone());
        }

        let response = self.sign_in.exchange(&self.http).await?;
        debug!(expires_in = response.expires_in, "Signed in to STACKIT");
        let lifetime = Duration::from_secs(response.expires_in).saturating_sub(RENEWAL_MARGIN);
        *current = Some(AccessToken {
            secret: response.access_token.clone(),
            renew_at: Instant::now() + lifetime,
        });
        Ok(response.access_token)
    }

    /// Forgets `refused`, which STACKIT no longer accepts, so that the next request signs in again. A token from a
    /// newer sign-in is kept.
    pub(crate) async fn forget(&self, refused: &str) {
        let mut current = self.current.lock().await;
        if current.as_ref().is_some_and(|token| token.secret == refused) {
            *current = None;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use aws_lc_rs::encoding::AsDer as _;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde_json::json;
    use std::path::Path;
    use wiremock::{
        matchers::{body_string_contains, method},
        Mock, MockServer, ResponseTemplate,
    };

    /// A file that holds a workload identity token, removed when dropped.
    pub(crate) struct TokenFile(pub(crate) PathBuf);

    impl TokenFile {
        pub(crate) fn new() -> Self {
            let path = std::env::temp_dir().join(format!("stackit-token-{}", Uuid::new_v4()));
            std::fs::write(&path, "workload-identity-token\n").unwrap();
            Self(path)
        }
    }

    impl Drop for TokenFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn workload_identity(token_file: &Path, server: &MockServer) -> AccessTokens {
        let sign_in = SignIn::WorkloadIdentity {
            token_file: token_file.to_path_buf(),
            service_account_email: "unicore-kms@sa.stackit.cloud".to_string(),
            token_url: Url::parse(&format!("{}/token", server.uri())).unwrap(),
        };
        AccessTokens::new(sign_in, reqwest::Client::new())
    }

    fn token(expires_in: u64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-token",
            "token_type": "Bearer",
            "expires_in": expires_in
        }))
    }

    async fn sign_ins(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    /// A key file with a new RSA key, whose token endpoint is `server`'s.
    fn key_file(server: &MockServer) -> ServiceAccountKey {
        let key = aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048).unwrap();
        let der = key.as_der().unwrap();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(der.as_ref())
        );
        serde_json::from_value(json!({
            "id": "key-id",
            "credentials": {
                "kid": "key-id",
                "iss": "unicore-kms@sa.stackit.cloud",
                "sub": "service-account-id",
                "aud": "https://stackit-service-account-prod.apps.01.cf.eu01.stackit.cloud",
                "privateKey": pem,
                "tokenEndpoint": format!("{}/token", server.uri())
            }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn exchanges_the_workload_identity_token_for_the_service_account() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains(
                "grant_type=client_credentials&client_assertion_type=urn%3Aschwarz%3Aparams%3Aoauth%3Aclient-assertion-type%3Aworkload-jwt&client_assertion=workload-identity-token&",
            ))
            .and(body_string_contains("client_id=unicore-kms%40sa.stackit.cloud"))
            .respond_with(token(900))
            .mount(&server)
            .await;
        let file = TokenFile::new();

        let token = workload_identity(&file.0, &server).get().await.unwrap();

        assert_eq!(token, "access-token");
    }

    #[tokio::test]
    async fn exchanges_a_signed_assertion_for_the_service_account_of_a_key_file() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion=",
            ))
            .respond_with(token(900))
            .mount(&server)
            .await;
        let tokens = AccessTokens::new(SignIn::KeyFile(key_file(&server)), reqwest::Client::new());

        let token = tokens.get().await.unwrap();

        assert_eq!(token, "access-token");
    }

    #[tokio::test]
    async fn keeps_a_token_that_does_not_expire_soon() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(token(900))
            .mount(&server)
            .await;
        let file = TokenFile::new();
        let tokens = workload_identity(&file.0, &server);
        tokens.get().await.unwrap();

        tokens.get().await.unwrap();

        assert_eq!(sign_ins(&server).await, 1);
    }

    #[tokio::test]
    async fn signs_in_again_when_the_token_is_about_to_expire() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(token(30)).mount(&server).await;
        let file = TokenFile::new();
        let tokens = workload_identity(&file.0, &server);
        tokens.get().await.unwrap();

        tokens.get().await.unwrap();

        assert_eq!(sign_ins(&server).await, 2);
    }

    #[tokio::test]
    async fn signs_in_again_after_forgetting_the_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(token(900))
            .mount(&server)
            .await;
        let file = TokenFile::new();
        let tokens = workload_identity(&file.0, &server);
        let refused = tokens.get().await.unwrap();
        tokens.forget(&refused).await;

        tokens.get().await.unwrap();

        assert_eq!(sign_ins(&server).await, 2);
    }

    #[tokio::test]
    async fn names_a_missing_workload_identity_token() {
        let server = MockServer::start().await;
        let tokens = workload_identity(Path::new("/var/run/secrets/no-such-token"), &server);

        let error = tokens.get().await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "The workload identity token can't be read from `/var/run/secrets/no-such-token`: No such file or \
             directory (os error 2)"
        );
    }

    #[tokio::test]
    async fn reports_a_refused_sign_in() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "invalid_client",
                "error_description": "No federated identity provider trusts this token"
            })))
            .mount(&server)
            .await;
        let file = TokenFile::new();

        let error = workload_identity(&file.0, &server).get().await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "STACKIT refused the sign-in (400): No federated identity provider trusts this token"
        );
    }

    #[test]
    fn does_not_show_the_private_key_when_debugged() {
        let key: ServiceAccountKey = serde_json::from_value(json!({
            "credentials": {
                "kid": "key-id",
                "iss": "unicore-kms@sa.stackit.cloud",
                "sub": "service-account-id",
                "aud": "https://stackit-service-account-prod.apps.01.cf.eu01.stackit.cloud",
                "privateKey": "-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----",
                "tokenEndpoint": "https://service-account.api.stackit.cloud/token"
            }
        }))
        .unwrap();

        let debugged = format!("{:?}", SignIn::KeyFile(key));

        assert!(!debugged.contains("secret"), "{debugged}");
    }
}
