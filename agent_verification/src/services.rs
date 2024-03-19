use std::sync::Arc;

use oid4vc_core::Subject;
use oid4vc_manager::RelyingPartyManager;

/// Verification services. This struct is used to generate authorization requests and validate authorization responses.
/// It also sends connection notifications on a successful authorization response.
pub struct VerificationServices {
    pub verifier: Arc<dyn Subject>,
    pub relying_party: RelyingPartyManager,
    pub client: reqwest::Client,
}

impl VerificationServices {
    pub fn new(verifier: Arc<dyn Subject>) -> Self {
        Self {
            verifier: verifier.clone(),
            relying_party: RelyingPartyManager::new([verifier]).unwrap(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn send_connection_notification(&self, notification_uri: &url::Url) -> Result<(), reqwest::Error> {
        self.client.post(notification_uri.clone()).send().await?;
        Ok(())
    }
}

#[cfg(feature = "test")]
pub mod test_utils {
    use agent_shared::secret_manager::secret_manager;

    use super::*;

    pub fn test_verification_services() -> Arc<VerificationServices> {
        Arc::new(VerificationServices::new(Arc::new(futures::executor::block_on(
            async { secret_manager().await },
        ))))
    }
}
