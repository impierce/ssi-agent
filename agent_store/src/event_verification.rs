use agent_authorization::domain::{
    access_token::{aggregate::AccessToken, event::AccessTokenEvent},
    authorization_code::{aggregate::AuthorizationCode, event::AuthorizationCodeEvent},
    client::{aggregate::Client, event::ClientEvent},
    oauth2_authorization_request::{aggregate::OAuth2AuthorizationRequest, event::OAuth2AuthorizationRequestEvent},
};
use agent_holder::{
    credential::{aggregate::Credential as HolderCredential, event::CredentialEvent as HolderCredentialEvent},
    offer::{aggregate::Offer as ReceivedOffer, event::OfferEvent as ReceivedOfferEvent},
    presentation::{aggregate::Presentation, event::PresentationEvent},
};
use agent_identity::{
    connection::{aggregate::Connection, event::ConnectionEvent},
    document::{aggregate::Document, event::DocumentEvent},
    profile::{aggregate::Profile, event::ProfileEvent},
    service::{aggregate::Service, event::ServiceEvent},
};
use agent_issuance::{
    credential::{aggregate::Credential as IssuanceCredential, event::CredentialEvent as IssuanceCredentialEvent},
    nonce::{aggregate::Nonce, event::NonceEvent},
    offer::{aggregate::Offer as IssuanceOffer, event::OfferEvent as IssuanceOfferEvent},
    public_offer::{aggregate::PublicOffer, event::PublicOfferEvent},
    server_config::{aggregate::ServerConfig, event::ServerConfigEvent},
    status_list::{aggregate::StatusListAggregate, event::StatusListEvent},
};
use agent_library::{
    catalog::{aggregate::Catalog, event::CatalogEvent},
    template::{aggregate::Template, event::TemplateEvent},
};
use agent_verification::authorization_request::{aggregate::AuthorizationRequest, event::AuthorizationRequestEvent};
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

type VerifyPayload = fn(&RawStoredEvent) -> Result<(), String>;

struct EventVerifier {
    aggregate_type: &'static str,
    verify: VerifyPayload,
}

pub fn verify_events(events: impl IntoIterator<Item = RawStoredEvent>) -> EventVerificationReport {
    let mut report = EventVerificationReport::default();

    for event in events {
        report.checked += 1;

        let Some(verifier) = event_verifier_for(&event.aggregate_type) else {
            report
                .incompatible
                .push(incompatible_event(event, "unknown aggregate type"));
            continue;
        };

        if let Err(reason) = (verifier.verify)(&event) {
            report.incompatible.push(incompatible_event(event, reason));
        }
    }

    report
}

fn event_verifier_for(aggregate_type: &str) -> Option<&'static EventVerifier> {
    event_verifiers()
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

fn event_verifiers() -> &'static [EventVerifier] {
    &[
        EventVerifier {
            aggregate_type: AccessToken::TYPE,
            verify: verify_aggregate_event::<AccessTokenEvent>,
        },
        EventVerifier {
            aggregate_type: AuthorizationCode::TYPE,
            verify: verify_aggregate_event::<AuthorizationCodeEvent>,
        },
        EventVerifier {
            aggregate_type: Client::TYPE,
            verify: verify_aggregate_event::<ClientEvent>,
        },
        EventVerifier {
            aggregate_type: OAuth2AuthorizationRequest::TYPE,
            verify: verify_aggregate_event::<OAuth2AuthorizationRequestEvent>,
        },
        EventVerifier {
            aggregate_type: Connection::TYPE,
            verify: verify_aggregate_event::<ConnectionEvent>,
        },
        EventVerifier {
            aggregate_type: Document::TYPE,
            verify: verify_aggregate_event::<DocumentEvent>,
        },
        EventVerifier {
            aggregate_type: Profile::TYPE,
            verify: verify_aggregate_event::<ProfileEvent>,
        },
        EventVerifier {
            aggregate_type: Service::TYPE,
            verify: verify_aggregate_event::<ServiceEvent>,
        },
        EventVerifier {
            aggregate_type: Template::TYPE,
            verify: verify_aggregate_event::<TemplateEvent>,
        },
        EventVerifier {
            aggregate_type: Catalog::TYPE,
            verify: verify_aggregate_event::<CatalogEvent>,
        },
        EventVerifier {
            aggregate_type: ServerConfig::TYPE,
            verify: verify_aggregate_event::<ServerConfigEvent>,
        },
        EventVerifier {
            aggregate_type: IssuanceCredential::TYPE,
            verify: verify_aggregate_event::<IssuanceCredentialEvent>,
        },
        EventVerifier {
            aggregate_type: IssuanceOffer::TYPE,
            verify: verify_aggregate_event::<IssuanceOfferEvent>,
        },
        EventVerifier {
            aggregate_type: PublicOffer::TYPE,
            verify: verify_aggregate_event::<PublicOfferEvent>,
        },
        EventVerifier {
            aggregate_type: Nonce::TYPE,
            verify: verify_aggregate_event::<NonceEvent>,
        },
        EventVerifier {
            aggregate_type: StatusListAggregate::TYPE,
            verify: verify_aggregate_event::<StatusListEvent>,
        },
        EventVerifier {
            aggregate_type: HolderCredential::TYPE,
            verify: verify_aggregate_event::<HolderCredentialEvent>,
        },
        EventVerifier {
            aggregate_type: Presentation::TYPE,
            verify: verify_aggregate_event::<PresentationEvent>,
        },
        EventVerifier {
            aggregate_type: ReceivedOffer::TYPE,
            verify: verify_aggregate_event::<ReceivedOfferEvent>,
        },
        EventVerifier {
            aggregate_type: AuthorizationRequest::TYPE,
            verify: verify_aggregate_event::<AuthorizationRequestEvent>,
        },
    ]
}
