use agent_identity::project_identity_event;
use agent_issuance::project_issuance_event;
use agent_library::project_library_event;
use agent_verification::project_verification_event;
use futures::StreamExt;
use shared_kernel::event_bus::{CloudEvent, EventBus, EventBusHandle, EventFilter};
use tracing::{debug, error};

/// Function signature for integration event projectors.
pub type IntegrationProjectorFn = fn(&CloudEvent) -> Option<CloudEvent>;

/// Core domain integration event projectors in standard evaluation order.
pub const CORE_INTEGRATION_PROJECTORS: &[IntegrationProjectorFn] = &[
    project_issuance_event,
    project_identity_event,
    project_library_event,
    project_verification_event,
];

/// Translates a single internal domain [`CloudEvent`] to a public [`CloudEvent`] by querying
/// a slice of projector functions in order until one returns `Some`.
#[must_use]
pub fn project_event(domain_ce: &CloudEvent, projectors: &[IntegrationProjectorFn]) -> Option<CloudEvent> {
    for projector in projectors {
        if let Some(integration_ce) = projector(domain_ce) {
            return Some(integration_ce);
        }
    }
    None
}

/// Translates a single internal domain [`CloudEvent`] to a public [`CloudEvent`] using the core projectors.
#[must_use]
pub fn project_core_event(domain_ce: &CloudEvent) -> Option<CloudEvent> {
    project_event(domain_ce, CORE_INTEGRATION_PROJECTORS)
}

/// Spawns the background task that projects internal core domain events into public integration events
/// using the core domain projectors.
pub fn start_core_integration_projector(domain_event_bus: EventBusHandle, integration_event_bus: EventBusHandle) {
    start_integration_projector_with(domain_event_bus, integration_event_bus, CORE_INTEGRATION_PROJECTORS);
}

/// Spawns the background task that projects internal core domain events into public integration events
/// using the supplied slice of projector functions.
pub fn start_integration_projector_with(
    domain_event_bus: EventBusHandle,
    integration_event_bus: EventBusHandle,
    projectors: &'static [IntegrationProjectorFn],
) {
    let mut stream = domain_event_bus.subscribe(EventFilter::default());
    tokio::spawn(async move {
        debug!("Integration projector started");

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(domain_ce) => {
                    if let Some(integration_ce) = project_event(&domain_ce, projectors) {
                        integration_event_bus.publish(integration_ce);
                    }
                }
                Err(err) => {
                    error!("Error on domain_event_bus in integration projector: {:?}", err);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn projects_credential_offered_with_caller_metadata() {
        let domain_ce = CloudEvent::new("com.impierce.unicore.credential-offer-created", "/services/offer")
            .with_subject("offer-123")
            .with_data(json!({
                "offer_id": "offer-123",
                "credential_ids": ["cred-abc"],
                "pre_authorized_code": "code-xyz"
            }))
            .with_caller(Some("user-1".into()), Some("user".into()));

        let projected = project_core_event(&domain_ce).expect("expected projected event");
        assert_eq!(
            projected.event_type,
            "com.impierce.unicore.issuance.credential.offered"
        );
        assert_eq!(projected.source, "/services/issuance");
        assert_eq!(projected.subject.as_deref(), Some("offer-123"));
        assert_eq!(projected.extension.callerid.as_deref(), Some("user-1"));
        assert_eq!(projected.extension.callertype.as_deref(), Some("user"));
    }

    #[test]
    fn projects_credential_issued() {
        let domain_ce = CloudEvent::new("com.impierce.unicore.signed-credential-created", "/services/credential")
            .with_subject("cred-abc")
            .with_data(json!({
                "credential_id": "cred-abc",
                "status": "issued"
            }));

        let projected = project_core_event(&domain_ce).expect("expected projected event");
        assert_eq!(projected.event_type, "com.impierce.unicore.issuance.credential.issued");
        assert_eq!(projected.source, "/services/issuance");
        assert_eq!(projected.subject.as_deref(), Some("cred-abc"));
    }

    #[test]
    fn projects_verification_events() {
        let req_ce = CloudEvent::new(
            "com.impierce.unicore.authorization-request-created",
            "/services/authorizationrequest",
        )
        .with_subject("req-123")
        .with_data(json!({
            "authorization_request": { "client_id": "client-456" }
        }));
        let projected_req = project_core_event(&req_ce).expect("expected projected event");
        assert_eq!(
            projected_req.event_type,
            "com.impierce.unicore.verification.presentation.requested"
        );
        assert_eq!(projected_req.subject.as_deref(), Some("req-123"));

        let verified_ce = CloudEvent::new(
            "com.impierce.unicore.oid-4-vp-authorization-response-verified",
            "/services/authorizationrequest",
        )
        .with_subject("req-123")
        .with_data(json!({ "validated": true }));
        let projected_verified = project_core_event(&verified_ce).expect("expected projected event");
        assert_eq!(
            projected_verified.event_type,
            "com.impierce.unicore.verification.presentation.verified"
        );

        let failed_ce = CloudEvent::new(
            "com.impierce.unicore.oid-4-vp-authorization-response-verified",
            "/services/authorizationrequest",
        )
        .with_subject("req-123")
        .with_data(json!({ "validated": false }));
        let projected_failed = project_core_event(&failed_ce).expect("expected projected event");
        assert_eq!(
            projected_failed.event_type,
            "com.impierce.unicore.verification.presentation.failed"
        );
    }

    #[test]
    fn projects_connection_events() {
        let ce = CloudEvent::new("com.impierce.unicore.connection-added", "/services/connection")
            .with_subject("conn-1")
            .with_data(json!({
                "connection_id": "conn-1",
                "url": "https://example.com/invitation"
            }));
        let projected = project_core_event(&ce).expect("expected projected event");
        assert_eq!(projected.event_type, "com.impierce.unicore.connection.invitation.created");
        assert_eq!(projected.source, "/services/connection");
        assert_eq!(projected.subject.as_deref(), Some("conn-1"));
    }

    #[test]
    fn projects_template_created() {
        let ce = CloudEvent::new("com.impierce.unicore.template-created", "/services/template")
            .with_subject("tmpl-1")
            .with_data(json!({
                "template_id": "tmpl-1",
                "title": "Diploma",
                "data_model": "open_badges_3-0"
            }));
        let projected = project_core_event(&ce).expect("expected projected event");
        assert_eq!(projected.event_type, "com.impierce.unicore.template.created");
        assert_eq!(projected.source, "/services/template");
        assert_eq!(projected.subject.as_deref(), Some("tmpl-1"));
    }

    #[test]
    fn drops_internal_unmapped_events() {
        let internal_ce = CloudEvent::new("com.impierce.unicore.nonce-created", "/services/nonce")
            .with_data(json!({ "nonce": "abc-123" }));

        assert!(project_core_event(&internal_ce).is_none());
    }

    #[test]
    fn project_event_with_custom_projectors() {
        fn custom_projector(ce: &CloudEvent) -> Option<CloudEvent> {
            if ce.event_type == "com.custom.ping" {
                Some(CloudEvent::new("com.custom.pong", "/services/custom"))
            } else {
                None
            }
        }

        let custom_projectors: &[IntegrationProjectorFn] = &[
            project_issuance_event,
            custom_projector,
        ];

        let ping = CloudEvent::new("com.custom.ping", "/services/ping");
        let projected = project_event(&ping, custom_projectors).expect("expected projected custom event");
        assert_eq!(projected.event_type, "com.custom.pong");
    }
}
