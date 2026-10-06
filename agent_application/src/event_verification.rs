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
pub use agent_store::event_verification::{EventVerificationError, EventVerificationReport, EventVerifier};
use agent_verification::authorization_request::aggregate::AuthorizationRequest;
use std::sync::LazyLock;

/// Statically initialized list of event verifiers for all core aggregates.
///
/// Used during application startup by persistent event stores (e.g. PostgreSQL, MongoDB)
/// to verify that all historical persisted events can still be deserialized into current domain
/// event definitions before the application transitions to the ready state.
static CORE_EVENT_VERIFIERS: LazyLock<[EventVerifier; 20]> = LazyLock::new(|| {
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
