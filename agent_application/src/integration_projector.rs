use agent_identity::ConnectionIntegrationEvent;
use agent_issuance::IssuanceIntegrationEvent;
use agent_library::TemplateIntegrationEvent;
use agent_verification::VerificationIntegrationEvent;
use futures::StreamExt;
use shared_kernel::{
    event_bus::{CloudEvent, EventBus, EventBusHandle, EventFilter},
    IntegrationEvent,
};
use tracing::{debug, error};

/// Spawns the background task that projects internal core domain events into public integration events.
pub fn start_core_integration_projector(domain_event_bus: EventBusHandle, integration_event_bus: EventBusHandle) {
    let mut stream = domain_event_bus.subscribe(EventFilter::default());
    tokio::spawn(async move {
        debug!("Core integration projector started");

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(domain_ce) => {
                    if let Some(integration_ce) = project_core_event(&domain_ce) {
                        integration_event_bus.publish(integration_ce);
                    }
                }
                Err(err) => {
                    error!("Error on domain_event_bus in core integration projector: {:?}", err);
                }
            }
        }
    });
}

/// Translates a single internal domain [`CloudEvent`] to a public [`CloudEvent`] carrying an [`IntegrationEvent`].
#[must_use]
pub fn project_core_event(domain_ce: &CloudEvent) -> Option<CloudEvent> {
    let data = domain_ce.data.as_ref()?;
    let caller_id = domain_ce.extension.callerid.clone();
    let caller_type = domain_ce.extension.callertype.clone();

    let mut result_ce = match domain_ce.event_type.as_str() {
        // Issuance domain events
        "com.impierce.unicore.credential-offer-created" => {
            let offer_id = data
                .get("offer_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| domain_ce.subject.clone())
                .unwrap_or_default();
            let credential_ids = data
                .get("credential_ids")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(ToString::to_string)).collect())
                .unwrap_or_default();
            let pre_authorized_code = data
                .get("pre_authorized_code")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);

            IssuanceIntegrationEvent::CredentialOffered {
                offer_id,
                credential_ids,
                pre_authorized_code,
            }
            .into_cloud_event("/services/issuance", caller_id, caller_type)
            .ok()?
        }
        "com.impierce.unicore.signed-credential-created" | "com.impierce.unicore.credential-signed" => {
            let credential_id = data
                .get("credential_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| domain_ce.subject.clone())
                .unwrap_or_default();
            let status = data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("issued")
                .to_string();

            IssuanceIntegrationEvent::CredentialIssued { credential_id, status }
                .into_cloud_event("/services/issuance", caller_id, caller_type)
                .ok()?
        }
        "com.impierce.unicore.credential-status-updated" => {
            let credential_id = data
                .get("credential_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| domain_ce.subject.clone())
                .unwrap_or_default();

            IssuanceIntegrationEvent::CredentialRevoked { credential_id }
                .into_cloud_event("/services/issuance", caller_id, caller_type)
                .ok()?
        }

        // Verification domain events
        "com.impierce.unicore.authorization-request-created" => {
            let request_id = domain_ce.subject.clone().unwrap_or_default();
            let client_id = data
                .get("authorization_request")
                .and_then(|ar| ar.get("client_id").or_else(|| ar.get("client_id_scheme")))
                .and_then(|v| v.as_str())
                .map(ToString::to_string);

            VerificationIntegrationEvent::PresentationRequested { request_id, client_id }
                .into_cloud_event("/services/verification", caller_id, caller_type)
                .ok()?
        }
        "com.impierce.unicore.oid-4-vp-authorization-response-verified"
        | "com.impierce.unicore.si-o-pv-2-authorization-response-verified" => {
            let request_id = domain_ce.subject.clone().unwrap_or_default();
            let validated = data.get("validated").and_then(|v| v.as_bool()).unwrap_or(true);

            if validated {
                VerificationIntegrationEvent::PresentationVerified {
                    request_id,
                    validated: true,
                }
                .into_cloud_event("/services/verification", caller_id, caller_type)
                .ok()?
            } else {
                VerificationIntegrationEvent::PresentationFailed {
                    request_id,
                    reason: Some("Verification failed".to_string()),
                }
                .into_cloud_event("/services/verification", caller_id, caller_type)
                .ok()?
            }
        }

        // Identity connection domain events
        "com.impierce.unicore.connection-added" => {
            let connection_id = data
                .get("connection_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| domain_ce.subject.clone())
                .unwrap_or_default();
            let url = data.get("url").and_then(|v| v.as_str()).map(ToString::to_string);

            ConnectionIntegrationEvent::InvitationCreated { connection_id, url }
                .into_cloud_event("/services/connection", caller_id, caller_type)
                .ok()?
        }
        "com.impierce.unicore.connection-synced" | "com.impierce.unicore.connection-changes-accepted" => {
            let connection_id = data
                .get("connection_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| domain_ce.subject.clone())
                .unwrap_or_default();

            ConnectionIntegrationEvent::ConnectionEstablished { connection_id }
                .into_cloud_event("/services/connection", caller_id, caller_type)
                .ok()?
        }

        // Library template domain events
        "com.impierce.unicore.template-created" => {
            let template_id = data
                .get("template_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| domain_ce.subject.clone())
                .unwrap_or_default();
            let title = data
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let data_model = data
                .get("data_model")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            TemplateIntegrationEvent::TemplateCreated {
                template_id,
                title,
                data_model,
            }
            .into_cloud_event("/services/template", caller_id, caller_type)
            .ok()?
        }
        "com.impierce.unicore.title-updated"
        | "com.impierce.unicore.display-updated"
        | "com.impierce.unicore.tags-updated"
        | "com.impierce.unicore.status-updated"
        | "com.impierce.unicore.visibility-updated"
        | "com.impierce.unicore.description-updated"
        | "com.impierce.unicore.type-updated" => {
            let template_id = data
                .get("template_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| domain_ce.subject.clone())
                .unwrap_or_default();
            let title = data.get("title").and_then(|v| v.as_str()).map(ToString::to_string);
            let status = data.get("status").and_then(|v| v.as_str()).map(ToString::to_string);

            TemplateIntegrationEvent::TemplateUpdated {
                template_id,
                title,
                status,
            }
            .into_cloud_event("/services/template", caller_id, caller_type)
            .ok()?
        }

        // Unmapped internal domain events are intentionally dropped
        _ => return None,
    };

    // Preserve the original event timestamp
    if let Some(occurred_at) = domain_ce.time {
        result_ce.time = Some(occurred_at);
    }

    Some(result_ce)
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
            "tech.impierce.unicore.issuance.credential.offered"
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
        assert_eq!(projected.event_type, "tech.impierce.unicore.issuance.credential.issued");
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
            "tech.impierce.unicore.verification.presentation.requested"
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
            "tech.impierce.unicore.verification.presentation.verified"
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
            "tech.impierce.unicore.verification.presentation.failed"
        );
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
        assert_eq!(projected.event_type, "tech.impierce.unicore.template.created");
        assert_eq!(projected.source, "/services/template");
        assert_eq!(projected.subject.as_deref(), Some("tmpl-1"));
    }

    #[test]
    fn drops_internal_unmapped_events() {
        let internal_ce = CloudEvent::new("com.impierce.unicore.nonce-created", "/services/nonce")
            .with_data(json!({ "nonce": "abc-123" }));

        assert!(project_core_event(&internal_ce).is_none());
    }
}
