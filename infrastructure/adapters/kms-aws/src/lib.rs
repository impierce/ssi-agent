//! AWS KMS key provider, authenticated with the credentials of the environment `UniCore` runs in ("ambient").
//!
//! Credentials and region are resolved through the standard AWS chain: environment variables, shared config files,
//! container or instance roles. Nothing about the AWS account is stored in `UniCore`.

use agent_kms::adapter::{ConnectionError, KeyProviderAdapter};
use async_trait::async_trait;
use aws_config::{timeout::TimeoutConfig, BehaviorVersion};
use aws_sdk_kms::{
    config::{retry::RetryConfig, Builder as KmsConfigBuilder},
    error::{ProvideErrorMetadata, SdkError},
    Client,
};
use std::time::Duration;
use tokio::sync::OnceCell;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AwsKmsAmbientKeyProvider {
    client: OnceCell<Client>,
}

impl AwsKmsAmbientKeyProvider {
    /// Creates the provider. The AWS configuration is only resolved on first use, so `UniCore` starts without AWS
    /// credentials as long as this provider isn't used.
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: OnceCell::new(),
        }
    }

    /// Creates the provider with a preconfigured client, e.g. one pointing at a test endpoint.
    #[must_use]
    pub fn with_client(client: Client) -> Self {
        Self {
            client: OnceCell::new_with(Some(client)),
        }
    }

    async fn client(&self) -> &Client {
        self.client
            .get_or_init(|| async {
                let config = aws_config::defaults(BehaviorVersion::latest())
                    .timeout_config(
                        TimeoutConfig::builder()
                            .connect_timeout(CONNECT_TIMEOUT)
                            .operation_timeout(OPERATION_TIMEOUT)
                            .build(),
                    )
                    .load()
                    .await;
                Client::new(&config)
            })
            .await
    }
}

impl Default for AwsKmsAmbientKeyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl KeyProviderAdapter for AwsKmsAmbientKeyProvider {
    /// Lists at most one key, which needs working credentials, a region and the `kms:ListKeys` permission. The
    /// check is not retried, so an unreachable provider is reported straight away.
    async fn test_connection(&self) -> Result<(), ConnectionError> {
        self.client()
            .await
            .list_keys()
            .limit(1)
            .customize()
            .config_override(KmsConfigBuilder::default().retry_config(RetryConfig::disabled()))
            .send()
            .await
            .map(|_| ())
            .map_err(|error| ConnectionError(describe(&error)))
    }
}

/// Describes why AWS could not be reached. AWS's own error code and message are clear enough to show as is; other
/// failures, such as missing credentials or network errors, need their full context to be understood.
fn describe<E, R>(error: &SdkError<E, R>) -> String
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
    R: std::fmt::Debug,
{
    match error.as_service_error().map(|error| (error.code(), error.message())) {
        Some((Some(code), Some(message))) => format!("{code}: {message}"),
        Some((Some(code), None)) => code.to_string(),
        _ => {
            let mut causes = vec![error.to_string()];
            let mut source = std::error::Error::source(error);
            while let Some(cause) = source {
                causes.push(cause.to_string());
                source = cause.source();
            }
            causes.join(": ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_kms::config::{Credentials, Region};
    use serde_json::json;
    use wiremock::{
        matchers::{header, method},
        Mock, MockBuilder, MockServer, ResponseTemplate,
    };

    fn provider(endpoint: &str) -> AwsKmsAmbientKeyProvider {
        let config = KmsConfigBuilder::default()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("eu-central-1"))
            .credentials_provider(Credentials::new("access-key", "secret-key", None, None, "test"))
            .endpoint_url(endpoint)
            .build();
        AwsKmsAmbientKeyProvider::with_client(Client::from_conf(config))
    }

    fn list_keys() -> MockBuilder {
        Mock::given(method("POST")).and(header("x-amz-target", "TrentService.ListKeys"))
    }

    #[tokio::test]
    async fn is_reachable_when_keys_can_be_listed() {
        let server = MockServer::start().await;
        list_keys()
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "Keys": [], "Truncated": false })))
            .expect(1)
            .mount(&server)
            .await;

        assert!(provider(&server.uri()).test_connection().await.is_ok());
    }

    #[tokio::test]
    async fn reports_why_the_provider_is_unreachable() {
        let server = MockServer::start().await;
        list_keys()
            .respond_with(
                ResponseTemplate::new(400)
                    .insert_header("x-amzn-ErrorType", "AccessDeniedException")
                    .set_body_json(json!({
                        "__type": "AccessDeniedException",
                        "message": "User is not authorized to perform: kms:ListKeys"
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = provider(&server.uri()).test_connection().await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "AccessDeniedException: User is not authorized to perform: kms:ListKeys"
        );
    }

    #[tokio::test]
    async fn reports_an_unreachable_endpoint_without_retrying() {
        let server = MockServer::start().await;
        list_keys()
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;

        assert!(provider(&server.uri()).test_connection().await.is_err());
    }
}
