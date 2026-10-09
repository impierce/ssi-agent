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
    offer::aggregate::Offer as IssuanceOffer, public_offer::aggregate::PublicOffer, reissuance::aggregate::Reissuance,
    server_config::aggregate::ServerConfig, status_list::aggregate::StatusListAggregate,
};
use agent_library::{catalog::aggregate::Catalog, template::aggregate::Template};
pub use agent_store::event_verification::{EventVerificationError, EventVerificationReport, EventVerifier};
use agent_verification::authorization_request::aggregate::AuthorizationRequest;
use std::sync::LazyLock;

/// Statically initialized list of event verifiers for all core aggregates.
///
/// Used during application startup by persistent event stores (e.g. PostgreSQL, MongoDB)
/// to verify that all historical persisted events can still be deserialized into current domain
/// event definitions before the application transitions to the ready state.
static CORE_EVENT_VERIFIERS: LazyLock<[EventVerifier; 21]> = LazyLock::new(|| {
    [
        EventVerifier::for_aggregate::<AccessToken>(),
        EventVerifier::for_aggregate::<AuthorizationCode>(),
        EventVerifier::for_aggregate::<Client>(),
        EventVerifier::for_aggregate::<OAuth2AuthorizationRequest>(),
        EventVerifier::for_aggregate::<Connection>(),
        EventVerifier::for_aggregate::<Document>(),
        EventVerifier::for_aggregate::<Profile>(),
        EventVerifier::for_aggregate::<Service>(),
        EventVerifier::for_aggregate::<Template>(),
        EventVerifier::for_aggregate::<Catalog>(),
        EventVerifier::for_aggregate::<ServerConfig>(),
        EventVerifier::for_aggregate::<IssuanceCredential>(),
        EventVerifier::for_aggregate::<Reissuance>(),
        EventVerifier::for_aggregate::<IssuanceOffer>(),
        EventVerifier::for_aggregate::<PublicOffer>(),
        EventVerifier::for_aggregate::<Nonce>(),
        EventVerifier::for_aggregate::<StatusListAggregate>(),
        EventVerifier::for_aggregate::<HolderCredential>(),
        EventVerifier::for_aggregate::<Presentation>(),
        EventVerifier::for_aggregate::<ReceivedOffer>(),
        EventVerifier::for_aggregate::<AuthorizationRequest>(),
    ]
});

/// Returns a slice of all registered core aggregate event verifiers.
pub fn core_event_verifiers() -> &'static [EventVerifier] {
    &CORE_EVENT_VERIFIERS[..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_issuance::reissuance::event::ReissuanceEvent;
    use agent_store::event_verification::{verify_events_with, RawStoredEvent};

    #[test]
    fn persisted_reissuance_events_are_recognized_by_core_verification() {
        let event = ReissuanceEvent::ReissuanceCreated {
            reissuance_id: "reissuance-id".to_string(),
            original_credential_id: "original-id".to_string(),
            new_credential_id: "new-id".to_string(),
            offer_id: "offer-id".to_string(),
            credential_configuration_id: "configuration-id".to_string(),
            reason: None,
            trigger_type: None,
            triggered_by: None,
            status_action: None,
            created_at: "2026-10-09T00:00:00Z".parse().unwrap(),
        };
        let report = verify_events_with(
            [RawStoredEvent {
                aggregate_type: "reissuance".to_string(),
                aggregate_id: "reissuance-id".to_string(),
                sequence: 1,
                event_type: "ReissuanceCreated".to_string(),
                event_version: "1".to_string(),
                payload: serde_json::to_value(event).unwrap(),
            }],
            core_event_verifiers(),
        );

        assert_eq!(report.checked, 1);
        assert!(report.is_compatible(), "{:?}", report.incompatible);
    }
}
