use crate::EventPublisher;
use agent_authorization::domain::{
    access_token::aggregate::AccessToken, authorization_code::aggregate::AuthorizationCode, client::aggregate::Client,
    oauth2_authorization_request::aggregate::OAuth2AuthorizationRequest,
};
use agent_holder::presentation::aggregate::Presentation;
use agent_identity::{
    connection::aggregate::Connection, document::aggregate::Document, profile::aggregate::Profile,
    service::aggregate::Service,
};
use agent_issuance::{
    credential::aggregate::Credential, nonce::aggregate::Nonce, offer::aggregate::Offer,
    public_offer::aggregate::PublicOffer, server_config::aggregate::ServerConfig,
    status_list::aggregate::StatusListAggregate,
};
use agent_library::template::aggregate::Template;
use agent_verification::authorization_request::aggregate::AuthorizationRequest;
use async_trait::async_trait;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use shared_kernel::event_bus::{build_cloud_event, EventBusHandle};

/// A `cqrs_es::Query` implementation that forwards committed aggregate events to the [`EventBusHandle`].
pub struct BusForwardingQuery<A: Aggregate> {
    bus: EventBusHandle,
    _phantom: std::marker::PhantomData<A>,
}

impl<A: Aggregate> BusForwardingQuery<A> {
    pub fn new(bus: EventBusHandle) -> Self {
        Self {
            bus,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<A> Query<A> for BusForwardingQuery<A>
where
    A: Aggregate,
    A::Event: serde::Serialize + DomainEvent,
{
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        for envelope in events {
            let payload = match serde_json::to_value(&envelope.payload) {
                Ok(val) => val,
                Err(err) => {
                    tracing::error!("Failed to serialize event payload for EventBus: {:?}", err);
                    continue;
                }
            };

            let occurred_at = envelope
                .metadata
                .get("occurred_at")
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc));

            let cloud_event = build_cloud_event(
                A::TYPE,
                aggregate_id,
                envelope.sequence,
                &envelope.payload.event_type(),
                payload,
                occurred_at,
            );

            self.bus.publish(cloud_event);
        }
    }
}

/// A struct that implements [`EventPublisher`] to forward all domain events
/// to the internal [`EventBusHandle`].
pub struct EventBusPublisher {
    bus: EventBusHandle,
}

impl EventBusPublisher {
    pub fn new(bus: EventBusHandle) -> Self {
        Self { bus }
    }
}

impl EventPublisher for EventBusPublisher {
    fn connection(&mut self) -> Option<crate::ConnectionEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Connection>::new(self.bus.clone())))
    }
    fn document(&mut self) -> Option<crate::DocumentEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Document>::new(self.bus.clone())))
    }
    fn profile(&mut self) -> Option<crate::ProfileEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Profile>::new(self.bus.clone())))
    }
    fn service(&mut self) -> Option<crate::ServiceEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Service>::new(self.bus.clone())))
    }
    fn template(&mut self) -> Option<crate::TemplateEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Template>::new(self.bus.clone())))
    }
    fn authorization_code(&mut self) -> Option<crate::AuthorizationCodeEventPublisher> {
        Some(Box::new(BusForwardingQuery::<AuthorizationCode>::new(self.bus.clone())))
    }
    fn client(&mut self) -> Option<crate::ClientEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Client>::new(self.bus.clone())))
    }
    fn oauth2_authorization_request(&mut self) -> Option<crate::OAuth2AuthorizationRequestEventPublisher> {
        Some(Box::new(BusForwardingQuery::<OAuth2AuthorizationRequest>::new(
            self.bus.clone(),
        )))
    }
    fn access_token(&mut self) -> Option<crate::AccessTokenEventPublisher> {
        Some(Box::new(BusForwardingQuery::<AccessToken>::new(self.bus.clone())))
    }
    fn server_config(&mut self) -> Option<crate::ServerConfigEventPublisher> {
        Some(Box::new(BusForwardingQuery::<ServerConfig>::new(self.bus.clone())))
    }
    fn credential(&mut self) -> Option<crate::CredentialEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Credential>::new(self.bus.clone())))
    }
    fn offer(&mut self) -> Option<crate::OfferEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Offer>::new(self.bus.clone())))
    }
    fn public_offer(&mut self) -> Option<crate::PublicOfferEventPublisher> {
        Some(Box::new(BusForwardingQuery::<PublicOffer>::new(self.bus.clone())))
    }
    fn nonce(&mut self) -> Option<crate::NonceEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Nonce>::new(self.bus.clone())))
    }
    fn status_list(&mut self) -> Option<crate::StatusListEventPublisher> {
        Some(Box::new(BusForwardingQuery::<StatusListAggregate>::new(
            self.bus.clone(),
        )))
    }
    fn holder_credential(&mut self) -> Option<crate::HolderCredentialEventPublisher> {
        Some(Box::new(BusForwardingQuery::<
            agent_holder::credential::aggregate::Credential,
        >::new(self.bus.clone())))
    }
    fn presentation(&mut self) -> Option<crate::PresentationEventPublisher> {
        Some(Box::new(BusForwardingQuery::<Presentation>::new(self.bus.clone())))
    }
    fn received_offer(&mut self) -> Option<crate::ReceivedOfferEventPublisher> {
        Some(Box::new(
            BusForwardingQuery::<agent_holder::offer::aggregate::Offer>::new(self.bus.clone()),
        ))
    }
    fn authorization_request(&mut self) -> Option<crate::AuthorizationRequestEventPublisher> {
        Some(Box::new(BusForwardingQuery::<AuthorizationRequest>::new(
            self.bus.clone(),
        )))
    }
}
