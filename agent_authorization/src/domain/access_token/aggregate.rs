use super::command::AccessTokenCommand;
use super::error::AccessTokenError;
use super::event::AccessTokenEvent;
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccessToken {
    #[serde(rename = "id")]
    pub access_token_id: String,
    pub user_id: String,
    pub client_id: String,
    pub scopes: Option<String>,
    pub issued_at: u64,
    pub access_token_expires_at: u64,
    pub refresh_token_expires_at: Option<u64>,
    pub issuer_state: Option<String>,
}

impl Aggregate for AccessToken {
    type Command = AccessTokenCommand;
    type Event = AccessTokenEvent;
    type Error = AccessTokenError;
    type Services = ();

    const TYPE: &'static str = "access_token";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use AccessTokenCommand::*;
        use AccessTokenEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
            IssueAccessToken {
                access_token_id,
                user_id,
                client_id,
                scopes,
                access_token_expires_in,
                refresh_token_expires_in,
                issuer_state,
            } => {
                #[cfg(not(test))]
                let issued_at = chrono::Utc::now().timestamp() as u64;
                #[cfg(test)]
                let issued_at = test_utils::issued_at();

                let access_token_expires_at = issued_at + access_token_expires_in;
                let refresh_token_expires_at = refresh_token_expires_in.map(|duration| issued_at + duration);

                Ok(vec![AccessTokenIssued {
                    access_token_id: access_token_id.clone(),
                    user_id,
                    client_id,
                    scopes,
                    issued_at,
                    access_token_expires_at,
                    refresh_token_expires_at,
                    issuer_state,
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use AccessTokenEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            AccessTokenIssued {
                access_token_id,
                user_id,
                client_id,
                scopes,
                issued_at,
                access_token_expires_at,
                refresh_token_expires_at,
                issuer_state,
            } => {
                self.access_token_id = access_token_id;
                self.user_id = user_id;
                self.client_id = client_id;
                self.scopes = scopes;
                self.issued_at = issued_at;
                self.access_token_expires_at = access_token_expires_at;
                self.refresh_token_expires_at = refresh_token_expires_at;
                self.issuer_state = issuer_state;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub mod token_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type AccessTokenTestFramework = TestFramework<AccessToken>;

    #[rstest]
    #[serial_test::serial]
    async fn test_issue_access_token(
        access_token_id: String,
        user_id: String,
        client_id: String,
        scopes: Option<String>,
        issued_at: u64,
        access_token_expires_in: u64,
        access_token_expires_at: u64,
        refresh_token_expires_in: Option<u64>,
        refresh_token_expires_at: Option<u64>,
        issuer_state: Option<String>,
    ) {
        AccessTokenTestFramework::with(())
            .given_no_previous_events()
            .when(AccessTokenCommand::IssueAccessToken {
                access_token_id: access_token_id.clone(),
                user_id: user_id.clone(),
                client_id: client_id.clone(),
                scopes: scopes.clone(),
                access_token_expires_in,
                refresh_token_expires_in,
                issuer_state: issuer_state.clone(),
            })
            .then_expect_events(vec![AccessTokenEvent::AccessTokenIssued {
                access_token_id,
                user_id,
                client_id,
                scopes,
                issued_at,
                access_token_expires_at,
                refresh_token_expires_at,
                issuer_state,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use rstest::*;

    #[fixture]
    pub fn access_token_id() -> String {
        "access_token_id".to_string()
    }

    #[fixture]
    pub fn user_id() -> String {
        "user_id".to_string()
    }

    #[fixture]
    pub fn client_id() -> String {
        "client_id".to_string()
    }

    #[fixture]
    pub fn scopes() -> Option<String> {
        Some("openid profile email".to_string())
    }

    #[fixture]
    pub fn issuer_state() -> Option<String> {
        Some("issuer_state".to_string())
    }

    #[fixture]
    pub fn issued_at() -> u64 {
        0
    }

    #[fixture]
    pub fn access_token_expires_in() -> u64 {
        3600 // 1 hour
    }

    #[fixture]
    pub fn access_token_expires_at(issued_at: u64, access_token_expires_in: u64) -> u64 {
        issued_at + access_token_expires_in
    }

    #[fixture]
    pub fn refresh_token_expires_in() -> Option<u64> {
        Some(86400) // 24 hours
    }

    #[fixture]
    pub fn refresh_token_expires_at(issued_at: u64, refresh_token_expires_in: Option<u64>) -> Option<u64> {
        refresh_token_expires_in.map(|duration| issued_at + duration)
    }
}
