use agent_authorization::domain::{
    access_token::aggregate::AccessToken, authorization_code::aggregate::AuthorizationCode, client::aggregate::Client,
    oauth2_authorization_request::aggregate::OAuth2AuthorizationRequest,
};
use agent_holder::{
    credential::aggregate::Credential as HolderCredential, offer::aggregate::Offer as ReceivedOffer,
    presentation::aggregate::Presentation,
};
use agent_identity::{
    connection::aggregate::Connection, document::aggregate::Document, profile::aggregate::Profile,
    service::aggregate::Service,
};
use agent_issuance::{
    credential::aggregate::Credential as IssuanceCredential, nonce::aggregate::Nonce,
    offer::aggregate::Offer as IssuanceOffer, public_offer::aggregate::PublicOffer,
    server_config::aggregate::ServerConfig, status_list::aggregate::StatusListAggregate,
};
use agent_library::{catalog::aggregate::Catalog, template::aggregate::Template};
use agent_verification::authorization_request::aggregate::AuthorizationRequest;
use cqrs_es::{Aggregate, DomainEvent};
use serde::de::DeserializeOwned;

#[derive(Clone, Debug)]
pub struct RawStoredEvent {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub event_version: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct IncompatibleEvent {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub event_version: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default)]
pub struct EventVerificationReport {
    pub checked: usize,
    pub incompatible: Vec<IncompatibleEvent>,
}

impl EventVerificationReport {
    pub fn is_compatible(&self) -> bool {
        self.incompatible.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EventVerificationError {
    #[error("failed to load persisted events from Postgres: {0}")]
    Postgres(#[from] sqlx::Error),
    #[error("failed to load persisted events from MongoDB: {0}")]
    MongoDb(#[from] mongodb::error::Error),
    #[error("MongoDB client has no default database configured")]
    MissingMongoDefaultDatabase,
    #[error("failed to deserialize persisted MongoDB event document: {0}")]
    MongoDbDocument(#[from] mongodb::bson::de::Error),
}

pub type VerifyPayload = fn(&RawStoredEvent) -> Result<(), String>;

#[derive(Clone, Copy)]
pub struct EventVerifier {
    aggregate_type: &'static str,
    verify: VerifyPayload,
}

impl EventVerifier {
    pub const fn new(aggregate_type: &'static str, verify: VerifyPayload) -> Self {
        Self { aggregate_type, verify }
    }

    pub fn for_aggregate<A>() -> Self
    where
        A: Aggregate,
        A::Event: DeserializeOwned,
    {
        Self {
            aggregate_type: A::TYPE,
            verify: verify_aggregate_event::<A::Event>,
        }
    }

    pub fn aggregate_type(&self) -> &'static str {
        self.aggregate_type
    }

    fn verify(&self, event: &RawStoredEvent) -> Result<(), String> {
        (self.verify)(event)
    }
}

pub fn verify_events(events: impl IntoIterator<Item = RawStoredEvent>) -> EventVerificationReport {
    verify_events_with(events, core_event_verifiers())
}

pub fn verify_events_with(
    events: impl IntoIterator<Item = RawStoredEvent>,
    verifiers: &[EventVerifier],
) -> EventVerificationReport {
    let mut report = EventVerificationReport::default();

    for event in events {
        report.checked += 1;

        let Some(verifier) = event_verifier_for(verifiers, &event.aggregate_type) else {
            report
                .incompatible
                .push(incompatible_event(event, "unknown aggregate type"));
            continue;
        };

        if let Err(reason) = verifier.verify(&event) {
            report.incompatible.push(incompatible_event(event, reason));
        }
    }

    report
}

fn event_verifier_for<'a>(verifiers: &'a [EventVerifier], aggregate_type: &str) -> Option<&'a EventVerifier> {
    verifiers
        .iter()
        .find(|verifier| verifier.aggregate_type == aggregate_type)
}

fn incompatible_event(event: RawStoredEvent, reason: impl Into<String>) -> IncompatibleEvent {
    IncompatibleEvent {
        aggregate_type: event.aggregate_type,
        aggregate_id: event.aggregate_id,
        sequence: event.sequence,
        event_type: event.event_type,
        event_version: event.event_version,
        reason: reason.into(),
    }
}

fn verify_aggregate_event<E>(raw: &RawStoredEvent) -> Result<(), String>
where
    E: DeserializeOwned + DomainEvent,
{
    let event: E = serde_json::from_value(raw.payload.clone()).map_err(|error| error.to_string())?;

    if event.event_type() != raw.event_type {
        return Err(format!(
            "event type mismatch: stored `{}`, current `{}`",
            raw.event_type,
            event.event_type()
        ));
    }

    if event.event_version() != raw.event_version {
        return Err(format!(
            "event version mismatch: stored `{}`, current `{}`",
            raw.event_version,
            event.event_version()
        ));
    }

    Ok(())
}

macro_rules! event_verifier {
    ($aggregate:ty) => {
        EventVerifier::new(
            <$aggregate as Aggregate>::TYPE,
            verify_aggregate_event::<<$aggregate as Aggregate>::Event>,
        )
    };
}

static CORE_EVENT_VERIFIERS: &[EventVerifier] = &[
    event_verifier!(AccessToken),
    event_verifier!(AuthorizationCode),
    event_verifier!(Client),
    event_verifier!(OAuth2AuthorizationRequest),
    event_verifier!(Connection),
    event_verifier!(Document),
    event_verifier!(Profile),
    event_verifier!(Service),
    event_verifier!(Template),
    event_verifier!(Catalog),
    event_verifier!(ServerConfig),
    event_verifier!(IssuanceCredential),
    event_verifier!(IssuanceOffer),
    event_verifier!(PublicOffer),
    event_verifier!(Nonce),
    event_verifier!(StatusListAggregate),
    event_verifier!(HolderCredential),
    event_verifier!(Presentation),
    event_verifier!(ReceivedOffer),
    event_verifier!(AuthorizationRequest),
];

pub fn core_event_verifiers() -> &'static [EventVerifier] {
    CORE_EVENT_VERIFIERS
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, error::Error, fmt};

    use cqrs_es::event_sink::EventSink;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    enum TestEvent {
        Created { id: String },
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "created".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    #[derive(Debug)]
    struct TestError;

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "test error")
        }
    }

    impl Error for TestError {}

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "test";

        type Command = Infallible;
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        async fn handle(
            &mut self,
            command: Self::Command,
            _service: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            match command {}
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    fn raw_event(aggregate_type: &str, payload: serde_json::Value) -> RawStoredEvent {
        RawStoredEvent {
            aggregate_type: aggregate_type.to_string(),
            aggregate_id: "aggregate-id".to_string(),
            sequence: 1,
            event_type: "created".to_string(),
            event_version: "1".to_string(),
            payload,
        }
    }

    #[test]
    fn core_verification_reports_unknown_external_aggregate() {
        let report = verify_events([raw_event("test", serde_json::json!({ "Created": { "id": "id" } }))]);

        assert_eq!(report.checked, 1);
        assert_eq!(report.incompatible.len(), 1);
        assert_eq!(report.incompatible[0].reason, "unknown aggregate type");
    }

    #[test]
    fn custom_verifier_accepts_external_aggregate() {
        let report = verify_events_with(
            [raw_event("test", serde_json::json!({ "Created": { "id": "id" } }))],
            &[EventVerifier::for_aggregate::<TestAggregate>()],
        );

        assert_eq!(report.checked, 1);
        assert!(report.is_compatible());
    }

    #[test]
    fn custom_verifier_reports_bad_payload() {
        let report = verify_events_with(
            [raw_event("test", serde_json::json!({ "Created": {} }))],
            &[EventVerifier::for_aggregate::<TestAggregate>()],
        );

        assert_eq!(report.checked, 1);
        assert_eq!(report.incompatible.len(), 1);
        assert!(report.incompatible[0].reason.contains("missing field"));
    }
}
