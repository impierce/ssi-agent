//! AWS KMS key provider, authenticated with the credentials of the environment `UniCore` runs in ("ambient").
//!
//! Credentials and region are resolved through the standard AWS chain: environment variables, shared config files,
//! container or instance roles. Nothing about the AWS account is stored in `UniCore`.

use agent_kms::adapter::{ConnectionError, KeyProviderAdapter, ProviderKey, ProviderKeyState, ProviderKeys};
use async_trait::async_trait;
use aws_config::{timeout::TimeoutConfig, BehaviorVersion};
use aws_sdk_kms::{
    config::{retry::RetryConfig, Builder as KmsConfigBuilder},
    error::{ProvideErrorMetadata, SdkError},
    types::KeyState,
    Client,
};
use futures::{stream, StreamExt as _, TryStreamExt as _};
use identity_kit::jose::JwsAlgorithm;
use std::{collections::HashMap, sync::RwLock, time::Duration};
use tokio::sync::OnceCell;
use tracing::debug;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
/// Upper bound for one whole request, including resolving the ambient AWS configuration and credentials on first
/// use, which the SDK's own timeouts don't cover.
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);
/// How many keys are described at the same time when listing keys.
const DESCRIBE_CONCURRENCY: usize = 8;

mod key_store;
pub use key_store::AwsKmsError;

#[cfg(test)]
mod fake_kms;

pub struct AwsKmsAmbientKeyProvider {
    client: OnceCell<Client>,
    deadline: Duration,
    /// The algorithm of every key looked up so far, by key id. A key's spec never changes in AWS KMS.
    algorithms: RwLock<HashMap<String, JwsAlgorithm>>,
    /// Tags given to every key this provider generates, for example to scope IAM policies to a tenant.
    key_tags: Vec<(String, String)>,
}

impl AwsKmsAmbientKeyProvider {
    /// Creates the provider. The AWS configuration is only resolved on first use, so `UniCore` starts without AWS
    /// credentials as long as this provider isn't used.
    #[must_use]
    pub fn new() -> Self {
        Self::with_cell(OnceCell::new())
    }

    fn with_cell(client: OnceCell<Client>) -> Self {
        Self {
            client,
            deadline: REQUEST_DEADLINE,
            algorithms: RwLock::default(),
            key_tags: Vec::new(),
        }
    }

    /// Creates the provider with a preconfigured client, e.g. one pointing at a test endpoint.
    #[must_use]
    pub fn with_client(client: Client) -> Self {
        Self::with_cell(OnceCell::new_with(Some(client)))
    }

    /// Tags every key this provider generates with `tags`, as key and value.
    #[must_use]
    pub fn with_key_tags(self, tags: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            key_tags: tags.into_iter().collect(),
            ..self
        }
    }

    /// Replaces the upper bound for one whole request.
    #[must_use]
    pub fn with_deadline(self, deadline: Duration) -> Self {
        Self { deadline, ..self }
    }

    /// Runs `request`, failing with a [`ConnectionError`] once the deadline passes.
    async fn within_deadline<T>(
        &self,
        request: impl std::future::Future<Output = Result<T, ConnectionError>>,
    ) -> Result<T, ConnectionError> {
        // The SDK's futures are large, so keep them on the heap rather than in every caller's future.
        tokio::time::timeout(self.deadline, Box::pin(request))
            .await
            .unwrap_or_else(|_| {
                Err(ConnectionError(format!(
                    "AWS KMS did not respond within {} seconds",
                    self.deadline.as_secs_f32()
                )))
            })
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
        self.within_deadline(async {
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
        })
        .await
    }

    /// Lists the keys these credentials may use.
    ///
    /// `kms:ListKeys` can't be limited in IAM and returns every key in the AWS account, including other tenants'
    /// keys when an account is shared. Each key is therefore described as well, and keys whose `kms:DescribeKey` is
    /// denied are left out and counted, so the IAM policy of the deployment decides which keys it sees. Keys
    /// deleted in the meantime are left out without being counted. Any other failure fails the whole listing rather
    /// than hiding keys.
    async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
        self.within_deadline(async {
            let client = self.client().await;
            let ids = list_key_ids(client).await?;
            let listed = ids.len();

            let described: Vec<Described> = stream::iter(ids)
                .map(|id| describe_key(client, id))
                .buffered(DESCRIBE_CONCURRENCY)
                .try_collect()
                .await?;

            let mut listing = ProviderKeys::default();
            for key in described {
                match key {
                    Described::Visible(key) => listing.keys.push(key),
                    Described::Hidden => listing.hidden_keys += 1,
                    Described::Gone => {}
                }
            }

            debug!(
                listed,
                visible = listing.keys.len(),
                hidden = listing.hidden_keys,
                "Listed AWS KMS keys"
            );
            Ok(listing)
        })
        .await
    }
}

/// Lists the IDs of every key in the account, following AWS's pagination.
async fn list_key_ids(client: &Client) -> Result<Vec<String>, ConnectionError> {
    let mut ids = Vec::new();
    let mut marker = None;
    loop {
        let page = client
            .list_keys()
            .set_marker(marker)
            .send()
            .await
            .map_err(|error| ConnectionError(describe(&error)))?;
        ids.extend(page.keys().iter().filter_map(|key| key.key_id().map(str::to_string)));
        match page.next_marker() {
            Some(next) if page.truncated() => marker = Some(next.to_string()),
            _ => return Ok(ids),
        }
    }
}

/// What describing one listed key showed.
enum Described {
    Visible(ProviderKey),
    /// These credentials may not describe the key.
    Hidden,
    /// The key was deleted after it was listed.
    Gone,
}

async fn describe_key(client: &Client, id: String) -> Result<Described, ConnectionError> {
    match client.describe_key().key_id(&id).send().await {
        Ok(output) => Ok(output.key_metadata().map_or(Described::Gone, |metadata| {
            Described::Visible(ProviderKey {
                id: metadata.key_id().to_string(),
                resource_name: metadata.arn().map(str::to_string),
                state: match metadata.key_state() {
                    Some(KeyState::Enabled) => ProviderKeyState::Enabled,
                    Some(KeyState::Disabled) => ProviderKeyState::Disabled,
                    Some(KeyState::PendingDeletion) => ProviderKeyState::PendingDeletion,
                    _ => ProviderKeyState::Unavailable,
                },
            })
        })),
        Err(error) if error.code() == Some("AccessDeniedException") => Ok(Described::Hidden),
        Err(error) if error.code() == Some("NotFoundException") => Ok(Described::Gone),
        Err(error) => Err(ConnectionError(describe(&error))),
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
        matchers::{body_partial_json, header, method},
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

    fn describe_key(id: &str) -> MockBuilder {
        Mock::given(method("POST"))
            .and(header("x-amz-target", "TrentService.DescribeKey"))
            .and(body_partial_json(json!({ "KeyId": id })))
    }

    fn arn(id: &str) -> String {
        format!("arn:aws:kms:eu-central-1:123456789012:key/{id}")
    }

    fn described(id: &str, state: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "KeyMetadata": { "KeyId": id, "Arn": arn(id), "KeyState": state }
        }))
    }

    fn aws_error(code: &str) -> ResponseTemplate {
        ResponseTemplate::new(400)
            .insert_header("x-amzn-ErrorType", code)
            .set_body_json(json!({ "__type": code, "message": format!("{code} for this key") }))
    }

    async fn account_with_keys(ids: &[&str]) -> MockServer {
        let server = MockServer::start().await;
        let keys: Vec<_> = ids.iter().map(|id| json!({ "KeyId": id, "KeyArn": arn(id) })).collect();
        list_keys()
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "Keys": keys, "Truncated": false })))
            .mount(&server)
            .await;
        server
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
    async fn lists_keys_across_pages() {
        let server = MockServer::start().await;
        list_keys()
            .and(body_partial_json(json!({ "Marker": "page-2" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Keys": [{ "KeyId": "key-2", "KeyArn": arn("key-2") }],
                "Truncated": false
            })))
            .mount(&server)
            .await;
        list_keys()
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Keys": [{ "KeyId": "key-1", "KeyArn": arn("key-1") }],
                "Truncated": true,
                "NextMarker": "page-2"
            })))
            .mount(&server)
            .await;
        for id in ["key-1", "key-2"] {
            describe_key(id)
                .respond_with(described(id, "Enabled"))
                .mount(&server)
                .await;
        }

        let ids: Vec<_> = provider(&server.uri())
            .list_keys()
            .await
            .unwrap()
            .keys
            .into_iter()
            .map(|key| key.id)
            .collect();

        assert_eq!(ids, ["key-1", "key-2"]);
    }

    #[tokio::test]
    async fn leaves_out_keys_these_credentials_may_not_describe() {
        let server = account_with_keys(&["mine", "other-tenant", "deleted"]).await;
        describe_key("mine")
            .respond_with(described("mine", "Enabled"))
            .mount(&server)
            .await;
        describe_key("other-tenant")
            .respond_with(aws_error("AccessDeniedException"))
            .mount(&server)
            .await;
        describe_key("deleted")
            .respond_with(aws_error("NotFoundException"))
            .mount(&server)
            .await;

        let listing = provider(&server.uri()).list_keys().await.unwrap();

        assert_eq!(
            listing,
            ProviderKeys {
                keys: vec![ProviderKey {
                    id: "mine".to_string(),
                    resource_name: Some(arn("mine")),
                    state: ProviderKeyState::Enabled,
                }],
                hidden_keys: 1,
            }
        );
    }

    #[tokio::test]
    async fn fails_the_listing_instead_of_hiding_a_key_on_other_errors() {
        let server = account_with_keys(&["mine", "broken"]).await;
        describe_key("mine")
            .respond_with(described("mine", "Enabled"))
            .mount(&server)
            .await;
        describe_key("broken")
            .respond_with(aws_error("InvalidArnException"))
            .mount(&server)
            .await;

        let error = provider(&server.uri()).list_keys().await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "InvalidArnException: InvalidArnException for this key"
        );
    }

    #[tokio::test]
    async fn reports_whether_each_key_can_be_used() {
        let states = [
            ("enabled", "Enabled", ProviderKeyState::Enabled),
            ("disabled", "Disabled", ProviderKeyState::Disabled),
            ("deleting", "PendingDeletion", ProviderKeyState::PendingDeletion),
            ("creating", "Creating", ProviderKeyState::Unavailable),
        ];
        let ids: Vec<_> = states.iter().map(|(id, _, _)| *id).collect();
        let server = account_with_keys(&ids).await;
        for (id, aws_state, _) in states {
            describe_key(id)
                .respond_with(described(id, aws_state))
                .mount(&server)
                .await;
        }

        let reported: Vec<_> = provider(&server.uri())
            .list_keys()
            .await
            .unwrap()
            .keys
            .into_iter()
            .map(|key| (key.id, key.state))
            .collect();

        let expected: Vec<_> = states
            .iter()
            .map(|(id, _, state)| ((*id).to_string(), *state))
            .collect();
        assert_eq!(reported, expected);
    }

    #[tokio::test]
    async fn gives_up_when_aws_does_not_respond_in_time() {
        let server = MockServer::start().await;
        list_keys()
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let provider = provider(&server.uri()).with_deadline(Duration::from_millis(200));

        let error = provider.test_connection().await.unwrap_err();

        assert_eq!(error.to_string(), "AWS KMS did not respond within 0.2 seconds");
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
