use agent_identity::connection::aggregate::Connection;
use agent_issuance::{
    credential::aggregate::Credential as IssuanceCredential, offer::aggregate::Offer as IssuanceOffer,
    reissuance::aggregate::Reissuance,
};
use agent_library::template::aggregate::Template;
use agent_verification::authorization_request::aggregate::AuthorizationRequest;
pub use shared_kernel::EventProjector;
use std::sync::LazyLock;

/// Statically initialized list of integration event projectors for all core aggregates
/// that emit public integration events.
///
/// Used by persistent event sources (e.g. MongoDB Change Streams and catch-up queries)
/// to deserialize stored domain events into `cqrs_es::EventEnvelope<A>` and project them
/// into public `CloudEvent`s.
static CORE_EVENT_PROJECTORS: LazyLock<[EventProjector; 6]> = LazyLock::new(|| {
    [
        EventProjector::for_aggregate::<IssuanceOffer>(),
        EventProjector::for_aggregate::<IssuanceCredential>(),
        EventProjector::for_aggregate::<Reissuance>(),
        EventProjector::for_aggregate::<Connection>(),
        EventProjector::for_aggregate::<Template>(),
        EventProjector::for_aggregate::<AuthorizationRequest>(),
    ]
});

/// Returns a slice of all registered core aggregate integration event projectors.
#[must_use]
pub fn core_event_projectors() -> &'static [EventProjector] {
    &CORE_EVENT_PROJECTORS[..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::connection::event::ConnectionEvent;
    use agent_issuance::{credential::event::CredentialEvent, offer::event::OfferEvent};
    use agent_library::template::event::{
        DataModel, Expiration, HolderType, Status as TemplateStatus, TemplateEvent, Visibility,
    };
    use agent_shared::config::Authorization;
    use agent_verification::authorization_request::event::AuthorizationRequestEvent;
    use shared_kernel::{cqrs_es::EventEnvelope, ProjectIntegrationEvent};
    use std::collections::HashMap;

    #[test]
    fn projects_offer_event_with_caller_metadata() {
        let event = OfferEvent::CredentialRequestVerified {
            offer_id: "offer-123".to_string(),
            subject_id: Some("did:example:holder".to_string()),
        };
        let mut metadata = HashMap::new();
        metadata.insert("callerid".to_string(), "user-1".to_string());
        metadata.insert("callertype".to_string(), "user".to_string());

        let envelope = EventEnvelope {
            aggregate_id: "offer-123".to_string(),
            sequence: 1,
            payload: event,
            metadata,
        };

        let projected = IssuanceOffer::project_integration_event(&envelope).expect("expected projected event");
        assert_eq!(projected.id, "offer:offer-123:1");
        assert!(projected.time.is_some());
        assert_eq!(
            projected.event_type,
            "com.impierce.unicore.issuance.credential.request_verified"
        );
        assert_eq!(projected.source, "/services/issuance");
        assert_eq!(projected.subject.as_deref(), Some("offer-123"));
        assert_eq!(projected.extension.callerid.as_deref(), Some("user-1"));
        assert_eq!(projected.extension.callertype.as_deref(), Some("user"));
    }

    #[test]
    fn projects_credential_issued() {
        let envelope = EventEnvelope {
            aggregate_id: "cred-abc".to_string(),
            sequence: 1,
            payload: CredentialEvent::SignedCredentialCreated {
                credential_id: "cred-abc".to_string(),
                signed_credential: serde_json::json!({}),
                notification_id: None,
            },
            metadata: HashMap::default(),
        };

        let projected = IssuanceCredential::project_integration_event(&envelope).expect("expected projected event");
        assert_eq!(projected.id, "credential:cred-abc:1");
        assert_eq!(projected.event_type, "com.impierce.unicore.issuance.credential.issued");
        assert_eq!(projected.source, "/services/issuance");
        assert_eq!(projected.subject.as_deref(), Some("cred-abc"));
    }

    #[test]
    fn projects_verification_events() {
        let verified_envelope = EventEnvelope {
            aggregate_id: "req-123".to_string(),
            sequence: 2,
            payload: AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
                id_token: "jwt".to_string(),
                state: Some("state-1".to_string()),
                validated: true,
            },
            metadata: HashMap::default(),
        };
        let projected_verified =
            AuthorizationRequest::project_integration_event(&verified_envelope).expect("expected projected event");
        assert_eq!(projected_verified.id, "authorization_request:req-123:2");
        assert_eq!(
            projected_verified.event_type,
            "com.impierce.unicore.verification.presentation.verified"
        );

        let failed_envelope = EventEnvelope {
            aggregate_id: "req-123".to_string(),
            sequence: 3,
            payload: AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
                id_token: "jwt".to_string(),
                state: None,
                validated: false,
            },
            metadata: HashMap::default(),
        };
        let projected_failed =
            AuthorizationRequest::project_integration_event(&failed_envelope).expect("expected projected event");
        assert_eq!(projected_failed.id, "authorization_request:req-123:3");
        assert_eq!(
            projected_failed.event_type,
            "com.impierce.unicore.verification.presentation.failed"
        );
    }

    #[test]
    fn projects_connection_events() {
        let envelope = EventEnvelope {
            aggregate_id: "conn-1".to_string(),
            sequence: 1,
            payload: ConnectionEvent::ConnectionAdded {
                connection_id: "conn-1".to_string(),
                display: None,
                url: "https://example.com/invitation".parse().unwrap(),
                dids: vec![],
                first_interacted_at: None,
                last_interacted_at: None,
                validations: vec![],
            },
            metadata: HashMap::default(),
        };
        let projected = Connection::project_integration_event(&envelope).expect("expected projected event");
        assert_eq!(
            projected.event_type,
            "com.impierce.unicore.connection.invitation.created"
        );
        assert_eq!(projected.source, "/services/connection");
        assert_eq!(projected.subject.as_deref(), Some("conn-1"));
    }

    #[test]
    fn projects_template_created() {
        let event = TemplateEvent::TemplateCreated {
            template_id: "tmpl-1".to_string(),
            source_template_id: None,
            title: "Diploma".to_string(),
            display: Box::new(None),
            data_model: DataModel::OpenBadges3_0,
            holder_type: HolderType::Individual,
            modified_at: "2026-01-01T00:00:00Z".to_string(),
            tags: None,
            status: TemplateStatus::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::default(),
            description: None,
            r#type: vec![],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: Authorization::default(),
        };
        let payload = serde_json::to_value(&event).unwrap();
        let projected = core_event_projectors()
            .iter()
            .find(|p| p.aggregate_type() == "template")
            .and_then(|p| p.project("tmpl-1", 1, payload, HashMap::default()))
            .expect("expected projected event");

        assert_eq!(projected.event_type, "com.impierce.unicore.template.created");
        assert_eq!(projected.source, "/services/template");
        assert_eq!(projected.subject.as_deref(), Some("tmpl-1"));
    }
}
