//! TEMPORARY: feasibility measurement harness for in-memory projections.
//! Run with:
//!   cargo test -p agent_api_http projection_size -- --nocapture --test-threads=1
#![allow(clippy::all, clippy::pedantic)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Counting;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size >= layout.size() {
            ALLOCATED.fetch_add(new_size - layout.size(), Ordering::Relaxed);
        } else {
            ALLOCATED.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

pub fn allocated() -> usize {
    ALLOCATED.load(Ordering::Relaxed)
}

/// Deep heap footprint of a value: clone it `n` times and measure the allocator delta.
pub fn heap_size<T: Clone>(value: &T, n: usize) -> f64 {
    let before = allocated();
    let mut sink: Vec<T> = Vec::with_capacity(n);
    let after_vec = allocated();
    for _ in 0..n {
        sink.push(value.clone());
    }
    let after = allocated();
    let total = after.saturating_sub(after_vec);
    std::hint::black_box(&sink);
    drop(sink);
    let _ = before;
    total as f64 / n as f64
}

pub fn json_len<T: serde::Serialize>(v: &T) -> usize {
    serde_json::to_vec(v).unwrap().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v0::issuance::credential_issuer::token_status_list::tests::create_test_signed_credential;
    use crate::v0::issuance::credentials::tests::{create_test_template, setup_library_state};
    use crate::v0::issuance::router;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use agent_store::issuance_state;
    use std::sync::Arc;

    fn row(label: &str, count: usize, json: usize, heap: f64) {
        println!(
            "{:<34} n={:<6} json={:>10} B ({:>8.2} KiB)  heap={:>10.0} B ({:>8.2} KiB)  json/item={:>8} B  heap/item={:>8.0} B",
            label,
            count,
            json,
            json as f64 / 1024.0,
            heap,
            heap / 1024.0,
            if count > 0 { json / count } else { 0 },
            if count > 0 { heap / count as f64 } else { 0.0 },
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn projection_size_issuance() {
        const N: usize = 8;

        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state.clone()));

        for _ in 0..N {
            create_test_signed_credential(&mut app, &issuance_state, &template_id).await;
        }

        println!("\n================ ISSUANCE PROJECTIONS (N={N} signed credentials) ================");

        let all_credentials = issuance_state
            .query
            .all_credentials
            .load("all_credentials")
            .await
            .unwrap();
        if let Some(v) = &all_credentials {
            row("AllCredentialsView", v.credentials.len(), json_len(v), heap_size(v, 50));
            if let Some((id, c)) = v.credentials.iter().next() {
                println!("   sample credential id={id}");
                println!(
                    "   sample credential json:\n{}",
                    serde_json::to_string_pretty(c).unwrap()
                );
            }
        }

        let all_offers = issuance_state.query.all_offers.load("all_offers").await.unwrap();
        if let Some(v) = &all_offers {
            row("AllOffersView", v.offers.len(), json_len(v), heap_size(v, 50));
            if let Some((_, o)) = v.offers.iter().next() {
                println!("   sample offer json bytes = {}", json_len(o));
                println!("   sample offer json:\n{}", serde_json::to_string_pretty(o).unwrap());
            }
        }

        let all_status_lists = issuance_state
            .query
            .all_status_lists
            .load("all_status_lists")
            .await
            .unwrap();
        if let Some(v) = &all_status_lists {
            row(
                "AllStatusListsView",
                v.status_lists.len(),
                json_len(v),
                heap_size(v, 20),
            );
            if let Some((_, s)) = v.status_lists.iter().next() {
                println!(
                    "   sample status_list: json={} B, used_indices={} ",
                    json_len(s),
                    s.used_indices.len()
                );
            }
        }

        let all_public_offers = issuance_state
            .query
            .all_public_offers
            .load("all_public_offers")
            .await
            .unwrap();
        if let Some(v) = &all_public_offers {
            row("AllPublicOffersView", 0, json_len(v), heap_size(v, 20));
        }

        let server_config = issuance_state
            .query
            .server_config
            .load(agent_issuance::state::SERVER_CONFIG_ID)
            .await
            .unwrap();
        if let Some(v) = &server_config {
            row("ServerConfigView", 1, json_len(v), heap_size(v, 50));
        }

        let all_templates = library_state.query.all_templates.load("all_templates").await.unwrap();
        if let Some(v) = &all_templates {
            row("AllTemplatesView", v.templates.len(), json_len(v), heap_size(v, 50));
            if let Some((_, t)) = v.templates.iter().next() {
                println!("   sample template json:\n{}", serde_json::to_string_pretty(t).unwrap());
            }
        }
        println!("================================================================================\n");
    }
}

#[cfg(test)]
mod scaling_tests {
    use super::*;
    use agent_identity::connection::aggregate::Connection;
    use agent_identity::connection::event::ConnectionEvent;
    use agent_identity::connection::views::all_connections::AllConnectionsView;
    use agent_identity::connection::views::ConnectionView;
    use agent_issuance::credential::aggregate::Credential;
    use agent_issuance::credential::command::CredentialCommand;
    use agent_issuance::credential::views::all_credentials::AllCredentialsView;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use agent_store::issuance_state;
    use cqrs_es::{EventEnvelope, View};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Instant;

    const SAMPLE_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6InVybjp1dWlkOjEyM2U0NTY3LWU4OWItMTJkMy1hNDU2LTQyNjYxNDE3NDAwMCIsInZjIjp7ImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4iLCJpZCI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiXSwiaWQiOiJ1cm46dXVpZDoxMjNlNDU2Ny1lODliLTEyZDMtYTQ1Ni00MjY2MTQxNzQwMDAiLCJuYW1lIjoiVmVyaWZpYWJsZSBDcmVkZW50aWFsIiwiaXNzdWVyIjp7Im5hbWUiOiJVbmlDb3JlIiwiaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9LCJAY29udGV4dCI6WyJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIseyJsb2dvX3VyaSI6eyJAaWQiOiJodHRwczovL3d3dy5pYW5hLm9yZy9hc3NpZ25tZW50cy9qd3QjbG9nb191cmkiLCJAdHlwZSI6IkBpZCJ9fV0sImlzc3VhbmNlRGF0ZSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwidmFsaWRGcm9tIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJjcmVkZW50aWFsU3RhdHVzIjp7InR5cGUiOiJzdGF0dXNsaXN0K2p3dCIsImlkIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjoxMjN9fSwic3RhdHVzIjp7InN0YXR1c19saXN0Ijp7InVyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCIsImlkeCI6MTIzfX19.LOTI6t7MmHb-cJVRZA_xh6S4lOfqkKjQScAVpMLuh5HlytNrg1Kp0-oiJQXjxfvxUfnaiDL-J6Q1V95XtjscBA";

    fn connection_envelope(id: &str, linked_credentials: usize) -> EventEnvelope<Connection> {
        let mut validations = vec![json!({
            "DomainLinkage": {
                "domain": "https://issuer.example-university.org/",
                "did": "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM",
                "result": {
                    "valid": true,
                    "error": null,
                    "last_validated_at": "2026-03-04T12:00:00Z"
                }
            }
        })];

        if linked_credentials > 0 {
            let credentials: Vec<_> = (0..linked_credentials)
                .map(|_| {
                    json!({
                        "credential": {
                            "@context": ["https://www.w3.org/2018/credentials/v1"],
                            "id": "urn:uuid:123e4567-e89b-12d3-a456-426614174000",
                            "type": ["VerifiableCredential"],
                            "credentialSubject": {
                                "id": "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM",
                                "name": "Example University",
                                "legalName": "Example University Foundation"
                            },
                            "issuer": "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM",
                            "issuanceDate": "2020-12-04T14:12:19Z"
                        },
                        "jwt": SAMPLE_JWT,
                        "result": { "valid": true, "error": null, "last_validated_at": "2026-03-04T12:00:00Z" }
                    })
                })
                .collect();

            validations.push(json!({
                "LinkedVp": {
                    "url": "https://issuer.example-university.org/presentation.jsonld",
                    "did": "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM",
                    "domain_linkage_valid": true,
                    "result": { "valid": true, "error": null, "last_validated_at": "2026-03-04T12:00:00Z" },
                    "credentials": credentials
                }
            }));
        }

        let payload: ConnectionEvent = serde_json::from_value(json!({
            "ConnectionAdded": {
                "connection_id": id,
                "display": {
                    "name": "Example University",
                    "locale": "en",
                    "logo": { "uri": "https://issuer.example-university.org/logo.png", "alt_text": "Example University Logo" }
                },
                "url": "https://issuer.example-university.org/",
                "dids": ["did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM"],
                "first_interacted_at": "2026-03-04T12:00:00Z",
                "last_interacted_at": "2026-03-04T12:00:00Z",
                "validations": validations
            }
        }))
        .expect("connection event fixture must deserialize");

        EventEnvelope {
            aggregate_id: id.to_string(),
            sequence: 1,
            payload,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn projection_size_connections() {
        println!("\n================ CONNECTION PROJECTIONS ================");
        for (label, linked) in [("plain (domain linkage only)", 0), ("with linked VP (2 creds)", 2)] {
            let mut single = ConnectionView::default();
            single.update(&connection_envelope("conn-0", linked));
            println!(
                "single Connection [{label}]: json={} B, heap={:.0} B",
                json_len(&single),
                heap_size(&single, 200)
            );

            for n in [10usize, 50] {
                let mut view = AllConnectionsView::default();
                for i in 0..n {
                    view.update(&connection_envelope(&format!("conn-{i}"), linked));
                }
                println!(
                    "AllConnectionsView [{label}] n={n}: json={} B ({:.2} KiB), heap={:.0} B ({:.2} KiB), heap/item={:.0} B",
                    json_len(&view),
                    json_len(&view) as f64 / 1024.0,
                    heap_size(&view, 20),
                    heap_size(&view, 20) / 1024.0,
                    heap_size(&view, 20) / n as f64
                );
            }
        }
        println!("=======================================================\n");
    }

    /// Measures the cost of the `ListAllQuery` read-modify-write pattern as the
    /// `all_credentials` view grows: every single event rewrites the whole view.
    #[tokio::test(flavor = "current_thread")]
    async fn all_view_write_cost() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        println!("\n================ ALL-VIEW WRITE COST (in-memory MemRepository) ================");
        let total = 2000usize;
        let bucket = 250usize;
        let mut last = Instant::now();
        for i in 0..total {
            let id = format!("cred-{i:06}");
            let command: CredentialCommand = serde_json::from_value(json!({
                "credential_id": id,
                "signed_credential": SAMPLE_JWT,
            }))
            .unwrap();
            agent_shared::handlers::command_handler(
                issuance_state.authorization_checker.clone(),
                None,
                &id,
                &issuance_state.command.credential,
                command,
            )
            .await
            .unwrap();

            if (i + 1) % bucket == 0 {
                let elapsed = last.elapsed();
                println!(
                    "  after {:>5} credentials: {:>8.3} ms/credential (bucket of {bucket})",
                    i + 1,
                    elapsed.as_secs_f64() * 1000.0 / bucket as f64
                );
                last = Instant::now();
            }
        }

        let view = issuance_state
            .query
            .all_credentials
            .load("all_credentials")
            .await
            .unwrap()
            .unwrap();
        println!(
            "  final AllCredentialsView: n={}, json={} B ({:.2} MiB), heap={:.0} B ({:.2} MiB)",
            view.credentials.len(),
            json_len(&view),
            json_len(&view) as f64 / (1024.0 * 1024.0),
            heap_size(&view, 5),
            heap_size(&view, 5) / (1024.0 * 1024.0)
        );

        // Compare against a plain in-memory HashMap update (no serde round-trip).
        let mut plain = AllCredentialsView::default();
        let start = Instant::now();
        for i in 0..total {
            let id = format!("cred-{i:06}");
            let payload: agent_issuance::credential::event::CredentialEvent = serde_json::from_value(json!({
                "SignedCredentialCreated": {
                    "credential_id": id,
                    "signed_credential": SAMPLE_JWT,
                    "notification_id": "notification_id"
                }
            }))
            .unwrap();
            let envelope: EventEnvelope<Credential> = EventEnvelope {
                aggregate_id: id,
                sequence: 1,
                payload,
                metadata: HashMap::new(),
            };
            plain.update(&envelope);
        }
        println!(
            "  plain HashMap update for {total} credentials: {:.3} ms total ({:.4} ms/credential)",
            start.elapsed().as_secs_f64() * 1000.0,
            start.elapsed().as_secs_f64() * 1000.0 / total as f64
        );
        println!("==============================================================================\n");
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;
    use agent_library::template::aggregate::Template;
    use agent_library::template::event::TemplateEvent;
    use agent_library::template::views::all_templates::AllTemplatesView;
    use cqrs_es::{EventEnvelope, View};
    use serde_json::json;
    use std::collections::HashMap;

    fn schema_with(properties: usize, verbose: bool) -> serde_json::Value {
        let mut props = serde_json::Map::new();
        for i in 0..properties {
            let name = format!("attribute_number_{i:02}");
            let value = if verbose {
                json!({
                    "type": "string",
                    "title": format!("Attribute number {i}"),
                    "description": format!("A reasonably descriptive explanation of what attribute number {i} means for the holder of this credential."),
                    "examples": [format!("example value {i}")]
                })
            } else {
                json!({ "type": "string" })
            };
            props.insert(name, value);
        }
        json!({
            "type": "object",
            "properties": props,
            "required": ["attribute_number_00"]
        })
    }

    fn template_view(id: &str, properties: usize, verbose: bool) -> Template {
        let payload: TemplateEvent = serde_json::from_value(json!({
            "TemplateCreated": {
                "template_id": id,
                "source_template_id": null,
                "title": "Bachelor of Science Diploma",
                "display": {
                    "name": "Bachelor of Science Diploma",
                    "logo": { "uri": "https://issuer.example-university.org/logo.png", "alt_text": "Example University" }
                },
                "data_model": "w3c_vc_data_model_v2-0",
                "holder_type": "individual",
                "modified_at": "2026-09-15T12:00:00Z",
                "tags": ["education", "diploma"],
                "status": "published",
                "visibility": "private",
                "credential_expiration": { "type": "never" },
                "description": "Issued to graduates of the Example University Bachelor of Science programme.",
                "type": ["VerifiableCredential", "OpenBadgeCredential"],
                "schema": schema_with(properties, verbose),
                "schema_properties_attributes": null,
                "holder_authorization": { "pre_authorized": true }
            }
        }))
        .expect("template event fixture must deserialize");

        let mut view = Template::default();
        view.update(&EventEnvelope {
            aggregate_id: id.to_string(),
            sequence: 1,
            payload,
            metadata: HashMap::new(),
        });
        view
    }

    #[test]
    fn projection_size_templates() {
        println!("\n================ TEMPLATE PROJECTIONS ================");
        for (label, props, verbose) in [
            ("small schema (3 attrs)", 3usize, false),
            ("medium schema (15 attrs)", 15, false),
            ("rich schema (15 attrs, documented)", 15, true),
            ("large schema (40 attrs, documented)", 40, true),
        ] {
            let view = template_view("tpl-0", props, verbose);
            println!(
                "single Template [{label}]: json={} B, heap={:.0} B",
                json_len(&view),
                heap_size(&view, 200)
            );
        }

        for (label, props, verbose) in [("medium schema", 15usize, false), ("large documented schema", 40, true)] {
            for n in [100usize, 500] {
                let mut all = AllTemplatesView::default();
                for i in 0..n {
                    all.templates
                        .insert(format!("tpl-{i}"), template_view(&format!("tpl-{i}"), props, verbose));
                }
                println!(
                    "AllTemplatesView [{label}] n={n}: json={:.2} KiB, heap={:.2} KiB ({:.0} B/item)",
                    json_len(&all) as f64 / 1024.0,
                    heap_size(&all, 10) / 1024.0,
                    heap_size(&all, 10) / n as f64
                );
            }
        }
        println!("=====================================================\n");
    }
}

#[cfg(test)]
mod event_stream_tests {
    use super::*;
    use agent_issuance::credential::aggregate::Credential;
    use agent_issuance::nonce::aggregate::Nonce;
    use agent_issuance::offer::aggregate::Offer;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::initialize;
    use agent_issuance::status_list::aggregate::StatusListAggregate;
    use agent_secret_manager::service::Service;
    use agent_store::{in_memory::InMemory, issuance_state, EventPublisher};
    use async_trait::async_trait;
    use cqrs_es::{Aggregate, EventEnvelope, Query};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct Stats {
        count: AtomicUsize,
        bytes: AtomicUsize,
    }

    struct Recorder<A: Aggregate> {
        stats: Arc<Stats>,
        _p: std::marker::PhantomData<A>,
    }

    #[async_trait]
    impl<A: Aggregate> Query<A> for Recorder<A> {
        async fn dispatch(&self, _aggregate_id: &str, events: &[EventEnvelope<A>]) {
            for event in events {
                self.stats.count.fetch_add(1, Ordering::Relaxed);
                self.stats.bytes.fetch_add(json_len(&event.payload), Ordering::Relaxed);
            }
        }
    }

    #[derive(Default)]
    struct RecordingPublisher {
        credential: Arc<Stats>,
        offer: Arc<Stats>,
        nonce: Arc<Stats>,
        status_list: Arc<Stats>,
    }

    macro_rules! none_publisher {
        ($($name:ident -> $ty:ty),* $(,)?) => {
            $(fn $name(&mut self) -> Option<$ty> { None })*
        };
    }

    impl EventPublisher for RecordingPublisher {
        fn credential(&mut self) -> Option<agent_store::CredentialEventPublisher> {
            Some(Box::new(Recorder::<Credential> {
                stats: self.credential.clone(),
                _p: std::marker::PhantomData,
            }))
        }
        fn offer(&mut self) -> Option<agent_store::OfferEventPublisher> {
            Some(Box::new(Recorder::<Offer> {
                stats: self.offer.clone(),
                _p: std::marker::PhantomData,
            }))
        }
        fn nonce(&mut self) -> Option<agent_store::NonceEventPublisher> {
            Some(Box::new(Recorder::<Nonce> {
                stats: self.nonce.clone(),
                _p: std::marker::PhantomData,
            }))
        }
        fn status_list(&mut self) -> Option<agent_store::StatusListEventPublisher> {
            Some(Box::new(Recorder::<StatusListAggregate> {
                stats: self.status_list.clone(),
                _p: std::marker::PhantomData,
            }))
        }

        none_publisher!(
            connection -> agent_store::ConnectionEventPublisher,
            document -> agent_store::DocumentEventPublisher,
            profile -> agent_store::ProfileEventPublisher,
            service -> agent_store::ServiceEventPublisher,
            template -> agent_store::TemplateEventPublisher,
            authorization_code -> agent_store::AuthorizationCodeEventPublisher,
            client -> agent_store::ClientEventPublisher,
            oauth2_authorization_request -> agent_store::OAuth2AuthorizationRequestEventPublisher,
            access_token -> agent_store::AccessTokenEventPublisher,
            server_config -> agent_store::ServerConfigEventPublisher,
            public_offer -> agent_store::PublicOfferEventPublisher,
            holder_credential -> agent_store::HolderCredentialEventPublisher,
            presentation -> agent_store::PresentationEventPublisher,
            received_offer -> agent_store::ReceivedOfferEventPublisher,
            authorization_request -> agent_store::AuthorizationRequestEventPublisher,
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn projection_size_event_stream() {
        use crate::v0::issuance::credential_issuer::token_status_list::tests::create_test_signed_credential;
        use crate::v0::issuance::credentials::tests::{create_test_template, setup_library_state};
        use crate::v0::issuance::router;

        const N: usize = 8;
        let publisher = RecordingPublisher::default();
        let credential = publisher.credential.clone();
        let offer = publisher.offer.clone();
        let nonce = publisher.nonce.clone();
        let status_list = publisher.status_list.clone();

        let issuance_state = Arc::new(
            issuance_state(
                &InMemory,
                IssuanceServices::default().await,
                vec![Box::new(publisher) as Box<dyn EventPublisher>],
            )
            .await,
        );
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state.clone()));

        for _ in 0..N {
            create_test_signed_credential(&mut app, &issuance_state, &template_id).await;
        }

        println!("\n================ EVENT STREAM (N={N} full issuance flows) ================");
        let mut total_events = 0;
        let mut total_bytes = 0;
        for (label, stats) in [
            ("credential", &credential),
            ("offer", &offer),
            ("nonce", &nonce),
            ("status_list", &status_list),
        ] {
            let c = stats.count.load(Ordering::Relaxed);
            let b = stats.bytes.load(Ordering::Relaxed);
            total_events += c;
            total_bytes += b;
            println!(
                "  {label:<14} events={c:<5} payload_bytes={b:<8} ({:.0} B/event, {:.1} events per flow)",
                if c > 0 { b as f64 / c as f64 } else { 0.0 },
                c as f64 / N as f64
            );
        }
        println!(
            "  TOTAL          events={total_events:<5} payload_bytes={total_bytes:<8} => {:.0} B and {:.1} events per issued credential",
            total_bytes as f64 / N as f64,
            total_events as f64 / N as f64
        );
        println!("=========================================================================\n");
    }
}

#[cfg(test)]
mod credential_payload_tests {
    use super::*;
    use agent_issuance::credential::aggregate::Credential;
    use agent_issuance::credential::event::CredentialEvent;
    use agent_issuance::credential::views::all_credentials::AllCredentialsView;
    use cqrs_es::{EventEnvelope, View};
    use serde_json::json;
    use std::collections::HashMap;

    /// Builds a realistic credential view: an unsigned payload with `attributes` claims plus a
    /// signed JWT whose length matches what signing that payload actually produces
    /// (base64url of header + payload + signature ~= 1.37x the VC JSON + ~300 B of envelope).
    fn credential_view(id: &str, attributes: usize) -> Credential {
        let mut subject = serde_json::Map::new();
        subject.insert(
            "id".into(),
            json!("did:key:z6MkiieyoLMSVsJAZv7Jje5wWSkDEymUgkyF8kbcrjZpX3qd"),
        );
        for i in 0..attributes {
            subject.insert(
                format!("attribute_number_{i:02}"),
                json!(format!("value for attribute number {i}")),
            );
        }
        let vc = json!({
            "@context": ["https://www.w3.org/2018/credentials/v1"],
            "id": "urn:uuid:123e4567-e89b-12d3-a456-426614174000",
            "type": ["VerifiableCredential", "OpenBadgeCredential"],
            "name": "Bachelor of Science Diploma",
            "issuer": { "name": "Example University", "id": "did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt" },
            "credentialSubject": subject,
        });
        let vc_len = serde_json::to_vec(&vc).unwrap().len();
        let jwt_len = (vc_len as f64 * 1.37) as usize + 300;
        let signed_jwt: String = "e".repeat(jwt_len);

        let credential_configuration = json!({
            "format": "jwt_vc_json",
            "credential_definition": { "type": ["VerifiableCredential", "OpenBadgeCredential"] },
            "cryptographic_binding_methods_supported": ["did:jwk", "did:key"],
            "credential_signing_alg_values_supported": ["ES256", "EdDSA"],
            "proof_types_supported": { "jwt": { "proof_signing_alg_values_supported": ["ES256", "EdDSA"] } },
            "credential_metadata": { "display": [{ "name": "Bachelor of Science Diploma" }] }
        });

        let created: CredentialEvent = serde_json::from_value(json!({
            "UnsignedCredentialCreated": {
                "credential_id": id,
                "data": { "raw": vc },
                "credential_configuration": credential_configuration,
                "notification_id": "notification_id",
                "created_at": "2026-09-15T12:00:00Z",
                "expires_at": null
            }
        }))
        .expect("unsigned credential fixture");

        let signed: CredentialEvent = serde_json::from_value(json!({
            "CredentialSigned": {
                "credential_id": id,
                "signed_credential": signed_jwt,
                "credential_status": {
                    "index": 4711,
                    "status": "VALID",
                    "status_list_url": "https://issuer.example-university.org/ietf-oauth-token-status-list/0"
                },
                "status": "Issued"
            }
        }))
        .expect("signed credential fixture");

        let mut view = Credential::default();
        for payload in [created, signed] {
            view.update(&EventEnvelope {
                aggregate_id: id.to_string(),
                sequence: 1,
                payload,
                metadata: HashMap::new(),
            });
        }
        view
    }

    #[test]
    fn projection_size_credentials_by_payload() {
        println!("\n================ CREDENTIAL PROJECTIONS BY PAYLOAD SIZE ================");
        for attributes in [2usize, 5, 10, 20] {
            let n = 500usize;
            let mut all = AllCredentialsView::default();
            for i in 0..n {
                all.credentials.insert(
                    format!("cred-{i:06}"),
                    credential_view(&format!("cred-{i:06}"), attributes),
                );
            }
            let json = json_len(&all);
            let heap = heap_size(&all, 5);
            println!(
                "{attributes:>3} attributes: json/credential={:>6} B   heap/credential={:>7.0} B   |  1k creds={:>6.1} MiB heap, 10k creds={:>6.1} MiB heap",
                json / n,
                heap / n as f64,
                heap / n as f64 * 1_000.0 / (1024.0 * 1024.0),
                heap / n as f64 * 10_000.0 / (1024.0 * 1024.0),
            );
        }
        println!("=======================================================================\n");
    }
}
