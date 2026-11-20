//! Signing in to STACKIT as a service account, for a short-lived access token.
//!
//! In a deployment, `UniCore` uses workload identity federation: it exchanges the token its host gives the workload it
//! runs as, and the service account trusts that workload's identity, so no secret is kept. The key flow, which signs
//! in with a service account key file, is only for local development and tests.

use crate::StackitError;
use async_trait::async_trait;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use url::Url;

/// Where STACKIT exchanges sign-in assertions for access tokens.
pub const TOKEN_URL: &str = "https://accounts.stackit.cloud/oauth/v2/token";
/// Where STACKIT's own SDKs look for the workload identity token when nothing else is configured.
pub const DEFAULT_FEDERATED_TOKEN_FILE: &str = "/var/run/secrets/stackit.cloud/serviceaccount/token";

/// An access token is renewed when it has less than this left, so a request never starts with one about to expire.
const RENEW_BEFORE_EXPIRY: Duration = Duration::from_mins(1);
/// How long a key flow assertion is valid. STACKIT's own examples use ten minutes.
const ASSERTION_LIFETIME: Duration = Duration::from_mins(10);

/// Hands out access tokens for the STACKIT API.
#[async_trait]
pub trait AccessTokens: Send + Sync {
    /// A token that is valid for at least a minute.
    ///
    /// # Errors
    ///
    /// Fails when `UniCore` can't sign in to STACKIT.
    async fn access_token(&self) -> Result<String, StackitError>;

    /// Forgets the current token, after STACKIT refused it, so the next request signs in again.
    async fn forget(&self);
}

/// A service account key, as in the key file STACKIT hands out. Its `Debug` output leaves the private key out.
#[derive(Clone, Deserialize)]
pub struct ServiceAccountKey {
    kid: String,
    iss: String,
    sub: String,
    aud: String,
    #[serde(rename = "privateKey")]
    private_key: String,
    #[serde(rename = "tokenEndpoint")]
    token_endpoint: Option<Url>,
}

impl ServiceAccountKey {
    /// Reads the key from the content of a STACKIT service account key file.
    ///
    /// # Errors
    ///
    /// Fails when the content isn't a key file that holds a private key.
    pub fn from_key_file(content: &str) -> Result<Self, StackitError> {
        #[derive(Deserialize)]
        struct KeyFile {
            credentials: ServiceAccountKey,
        }
        serde_json::from_str::<KeyFile>(content)
            .map(|file| file.credentials)
            .map_err(|error| StackitError(format!("The STACKIT service account key file can't be read: {error}")))
    }
}

impl fmt::Debug for ServiceAccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceAccountKey")
            .field("kid", &self.kid)
            .field("iss", &self.iss)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
enum Grant {
    WorkloadIdentity {
        token_file: PathBuf,
        service_account_email: String,
    },
    KeyFile(ServiceAccountKey),
}

struct Cached {
    token: String,
    expires_at: Instant,
}

/// Signs in to STACKIT and keeps the access token until it is about to expire. STACKIT hands out no refresh token,
/// so it signs in again then.
pub struct TokenExchange {
    http: reqwest::Client,
    token_url: Url,
    grant: Grant,
    cached: Mutex<Option<Cached>>,
}

impl TokenExchange {
    /// Signs in as `service_account_email` with the workload identity token in `token_file`. The file is read for
    /// every sign-in, as the host renews the token in it.
    #[must_use]
    pub fn workload_identity(
        http: reqwest::Client,
        token_file: PathBuf,
        service_account_email: impl Into<String>,
    ) -> Self {
        Self::new(
            http,
            Grant::WorkloadIdentity {
                token_file,
                service_account_email: service_account_email.into(),
            },
        )
    }

    /// Signs in with a service account key. Only for local development and tests.
    #[must_use]
    pub fn key_file(http: reqwest::Client, key: ServiceAccountKey) -> Self {
        let token_url = key.token_endpoint.clone();
        let exchange = Self::new(http, Grant::KeyFile(key));
        match token_url {
            Some(token_url) => exchange.with_token_url(token_url),
            None => exchange,
        }
    }

    fn new(http: reqwest::Client, grant: Grant) -> Self {
        Self {
            http,
            token_url: Url::parse(TOKEN_URL).expect("the STACKIT token URL is a URL"),
            grant,
            cached: Mutex::default(),
        }
    }

    /// Signs in at `token_url` instead of STACKIT's, such as a test server.
    #[must_use]
    pub fn with_token_url(self, token_url: Url) -> Self {
        Self { token_url, ..self }
    }

    async fn sign_in(&self) -> Result<Cached, StackitError> {
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: u64,
        }
        let form = match &self.grant {
            Grant::WorkloadIdentity {
                token_file,
                service_account_email,
            } => {
                let token = tokio::fs::read_to_string(token_file).await.map_err(|error| {
                    StackitError(format!(
                        "The workload identity token at `{}` can't be read: {error}",
                        token_file.display()
                    ))
                })?;
                vec![
                    ("grant_type", "client_credentials".to_string()),
                    (
                        "client_assertion_type",
                        "urn:schwarz:params:oauth:client-assertion-type:workload-jwt".to_string(),
                    ),
                    ("client_assertion", token.trim().to_string()),
                    ("client_id", service_account_email.clone()),
                ]
            }
            Grant::KeyFile(key) => vec![
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer".to_string()),
                ("assertion", key_flow_assertion(key)?),
            ],
        };

        let response = self
            .http
            .post(self.token_url.clone())
            .form(&form)
            .send()
            .await
            .map_err(|error| StackitError(format!("STACKIT's sign-in can't be reached: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(StackitError(format!(
                "STACKIT refused the sign-in ({status}): {}",
                body.chars().take(crate::SHOWN_ANSWER_LENGTH).collect::<String>()
            )));
        }

        let TokenResponse {
            access_token,
            expires_in,
        } = response
            .json()
            .await
            .map_err(|error| StackitError(format!("STACKIT's sign-in answered unexpectedly: {error}")))?;
        Ok(Cached {
            token: access_token,
            expires_at: Instant::now() + Duration::from_secs(expires_in),
        })
    }
}

#[async_trait]
impl AccessTokens for TokenExchange {
    /// The lock is held while signing in, so that requests that find the token expired wait for one sign-in instead
    /// of each signing in. Each sign-in is bounded by the HTTP client's timeout.
    async fn access_token(&self) -> Result<String, StackitError> {
        let mut cached = self.cached.lock().await;
        if let Some(current) = cached
            .as_ref()
            .filter(|current| current.expires_at.saturating_duration_since(Instant::now()) > RENEW_BEFORE_EXPIRY)
        {
            return Ok(current.token.clone());
        }
        let renewed = self.sign_in().await?;
        let token = renewed.token.clone();
        *cached = Some(renewed);
        Ok(token)
    }

    async fn forget(&self) {
        *self.cached.lock().await = None;
    }
}

/// The JWT a service account key signs to sign in with the key flow.
fn key_flow_assertion(key: &ServiceAccountKey) -> Result<String, StackitError> {
    #[derive(Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        sub: &'a str,
        aud: &'a str,
        jti: String,
        iat: u64,
        exp: u64,
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| StackitError(error.to_string()))?
        .as_secs();
    let mut header = Header::new(Algorithm::RS512);
    header.kid = Some(key.kid.clone());
    let claims = Claims {
        iss: &key.iss,
        sub: &key.sub,
        aud: &key.aud,
        jti: uuid::Uuid::new_v4().to_string(),
        iat: now,
        exp: now + ASSERTION_LIFETIME.as_secs(),
    };
    let signing_key = EncodingKey::from_rsa_pem(key.private_key.as_bytes())
        .map_err(|error| StackitError(format!("The service account's private key can't be read: {error}")))?;
    jsonwebtoken::encode(&header, &claims, &signing_key)
        .map_err(|error| StackitError(format!("The sign-in assertion can't be signed: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_string_contains, method},
        Mock, MockServer, ResponseTemplate,
    };

    async fn sign_in_server(expires_in: u64) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "access", "expires_in": expires_in })),
            )
            .mount(&server)
            .await;
        server
    }

    fn token_file(content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("stackit-token-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, content).unwrap();
        path
    }

    fn exchange(server: &MockServer, token_file: PathBuf) -> TokenExchange {
        TokenExchange::workload_identity(reqwest::Client::new(), token_file, "unicore@sa.stackit.cloud")
            .with_token_url(Url::parse(&server.uri()).unwrap())
    }

    #[tokio::test]
    async fn exchanges_the_workload_identity_token_for_the_service_account() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains(
                "client_assertion_type=urn%3Aschwarz%3Aparams%3Aoauth%3Aclient-assertion-type%3Aworkload-jwt",
            ))
            .and(body_string_contains("client_assertion=workload-token&"))
            .and(body_string_contains("client_id=unicore%40sa.stackit.cloud"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "access", "expires_in": 900 })),
            )
            .mount(&server)
            .await;

        let token = exchange(&server, token_file("workload-token\n")).access_token().await;

        assert_eq!(token.unwrap(), "access");
    }

    #[tokio::test]
    async fn keeps_a_token_that_does_not_expire_soon() {
        let server = sign_in_server(900).await;
        let tokens = exchange(&server, token_file("workload-token"));
        tokens.access_token().await.unwrap();

        tokens.access_token().await.unwrap();

        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn signs_in_again_when_the_token_is_about_to_expire() {
        let server = sign_in_server(30).await;
        let tokens = exchange(&server, token_file("workload-token"));
        tokens.access_token().await.unwrap();

        tokens.access_token().await.unwrap();

        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn signs_in_again_after_forgetting_the_token() {
        let server = sign_in_server(900).await;
        let tokens = exchange(&server, token_file("workload-token"));
        tokens.access_token().await.unwrap();
        tokens.forget().await;

        tokens.access_token().await.unwrap();

        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn names_a_missing_workload_identity_token() {
        let server = sign_in_server(900).await;
        let tokens = exchange(&server, PathBuf::from("/nonexistent/stackit-token"));

        let error = tokens.access_token().await.unwrap_err();

        assert!(error.to_string().contains("`/nonexistent/stackit-token`"), "{error}");
    }

    #[tokio::test]
    async fn reports_a_refused_sign_in() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string(r#"{"error":"invalid_client"}"#))
            .mount(&server)
            .await;

        let error = exchange(&server, token_file("workload-token"))
            .access_token()
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            r#"STACKIT refused the sign-in (401 Unauthorized): {"error":"invalid_client"}"#
        );
    }

    #[test]
    fn does_not_show_the_private_key_when_debugged() {
        let key = ServiceAccountKey::from_key_file(
            r#"{ "credentials": { "kid": "k", "iss": "i", "sub": "s", "aud": "a", "privateKey": "secret" } }"#,
        )
        .unwrap();

        assert!(!format!("{key:?}").contains("secret"));
    }
}
