use crate::event_bus::CloudEvent;
use cqrs_es::{Aggregate, EventEnvelope};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Static documentation metadata for a group of [`IntegrationEvent`] variants (typically one enum per bounded context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegrationEventGroupDoc {
    pub name: &'static str,
    pub source: &'static str,
    pub description: &'static str,
    pub variants: &'static [IntegrationEventVariantDoc],
}

/// Static documentation metadata for a single [`IntegrationEvent`] enum variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegrationEventVariantDoc {
    pub variant_name: &'static str,
    pub event_type: &'static str,
    pub description: &'static str,
    pub subject_field: Option<&'static str>,
    pub fields: &'static [IntegrationEventFieldDoc],
}

/// Static documentation metadata for a payload field of an [`IntegrationEvent`] variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegrationEventFieldDoc {
    pub name: &'static str,
    pub ty: &'static str,
    pub required: bool,
    pub description: &'static str,
}

/// Renders a Markdown summary table of all integration events across the provided groups.
#[must_use]
pub fn render_integration_events_summary_table(groups: &[IntegrationEventGroupDoc]) -> String {
    let mut md = String::new();
    md.push_str("| Event Type | Source | Subject | Description |\n");
    md.push_str("| :--- | :--- | :--- | :--- |\n");
    for group in groups {
        for variant in group.variants {
            let subject = variant
                .subject_field
                .map_or_else(|| "—".to_string(), |field| format!("`{field}`"));
            let summary = first_paragraph(variant.description);
            let _ = writeln!(
                md,
                "| `{}` | `{}` | {} | {} |",
                variant.event_type, group.source, subject, summary
            );
        }
    }
    md
}

/// Renders the detailed per-group and per-variant payload sections for the provided integration event groups.
#[must_use]
pub fn render_integration_events_groups_markdown(groups: &[IntegrationEventGroupDoc]) -> String {
    let mut md = String::new();
    for group in groups {
        let _ = writeln!(md, "### `{}` (`{}`)\n", group.name, group.source);
        if !group.description.is_empty() {
            let _ = writeln!(md, "{}\n", group.description);
        }

        for variant in group.variants {
            let _ = writeln!(md, "#### `{}`\n", variant.event_type);
            if !variant.description.is_empty() {
                let _ = writeln!(md, "{}\n", variant.description);
            }
            let _ = writeln!(md, "- **Variant**: `{}::{}`", group.name, variant.variant_name);
            let _ = writeln!(md, "- **CloudEvent `source`**: `{}`", group.source);
            if let Some(subject) = variant.subject_field {
                let _ = writeln!(md, "- **CloudEvent `subject`**: `{subject}`\n");
            } else {
                md.push_str("- **CloudEvent `subject`**: *none*\n\n");
            }

            if !variant.fields.is_empty() {
                md.push_str("| Field | Type | Required | Description |\n");
                md.push_str("| :--- | :--- | :--- | :--- |\n");
                for field in variant.fields {
                    let req = if field.required { "Yes" } else { "No" };
                    let desc = single_line(field.description);
                    let _ = writeln!(md, "| `{}` | `{}` | {} | {} |", field.name, field.ty, req, desc);
                }
                md.push('\n');
            }
        }
    }
    md
}

/// Renders a complete Markdown reference document for the provided integration event groups.
#[must_use]
pub fn render_integration_events_markdown(title: &str, groups: &[IntegrationEventGroupDoc]) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# {title}\n");
    md.push_str(
        "<!-- This file is auto-generated from inline `///` doc comments on `#[derive(IntegrationEvent)]` enums. -->\n",
    );
    md.push_str(
        "<!-- Do not edit manually; run `cargo test generate_integration_events_docs -- --ignored` to regenerate. -->\n\n",
    );
    md.push_str(
        "All integration events are published as [CloudEvents v1.0](https://cloudevents.io/) JSON envelopes (`application/cloudevents+json`).\n\n",
    );
    md.push_str("## Event Catalog\n\n");
    md.push_str(&render_integration_events_groups_markdown(groups));

    md
}

fn first_paragraph(text: &str) -> String {
    let paragraph = text.split("\n\n").next().unwrap_or_default();
    single_line(paragraph)
}

fn single_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A formal Integration Event in the Published Language (PL).
///
/// Unlike internal CQRS domain events which represent private aggregate state changes,
/// integration events establish a stable contract for external consumers,
/// webhook subscribers, and cross-system streaming.
pub trait IntegrationEvent: Serialize + Send + Sync + 'static {
    /// [`CloudEvent`] `source` URI for all variants of this integration event enum (e.g. `"/services/issuance"`).
    const SOURCE: &'static str;

    /// Compile-time documentation metadata for this integration event group.
    const DOCS: IntegrationEventGroupDoc;

    /// Reverse-DNS event type, e.g. `com.impierce.unicore.issuance.credential.offered`.
    fn event_type(&self) -> &'static str;

    /// Optional entity or aggregate subject ID.
    fn subject(&self) -> Option<String>;

    /// Converts this integration event into a CNCF [`CloudEvent`] v1.0 using [`Self::SOURCE`].
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialization of `self` into a JSON value fails.
    fn into_cloud_event(
        self,
        caller_id: Option<String>,
        caller_type: Option<String>,
    ) -> Result<CloudEvent, serde_json::Error>
    where
        Self: Sized,
    {
        let serialized = serde_json::to_value(&self)?;
        let subject = self.subject();
        let event_type = self.event_type();
        let data = match serialized {
            serde_json::Value::Object(mut map)
                if map.get("type").and_then(serde_json::Value::as_str) == Some(event_type)
                    && (map.len() == 1 || (map.len() == 2 && map.contains_key("data"))) =>
            {
                map.remove("data").unwrap_or_else(|| serde_json::json!({}))
            }
            other => other,
        };
        let mut ce = CloudEvent::new(event_type, Self::SOURCE)
            .with_data(data)
            .with_caller(caller_id, caller_type);
        if let Some(s) = subject {
            ce = ce.with_subject(s);
        }
        Ok(ce)
    }

    /// Deserializes a typed [`IntegrationEvent`] from a [`CloudEvent`] envelope.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if the [`CloudEvent`] `type` or `data` payload does not match `Self`.
    fn from_cloud_event(event: &CloudEvent) -> Result<Self, serde_json::Error>
    where
        Self: DeserializeOwned,
    {
        serde_json::from_value(serde_json::json!({
            "type": event.event_type,
            "data": event.data.clone().unwrap_or(serde_json::Value::Null),
        }))
        .or_else(|err| match &event.data {
            None => serde_json::from_value(serde_json::json!({ "type": event.event_type })),
            Some(serde_json::Value::Object(map)) if map.is_empty() => {
                serde_json::from_value(serde_json::json!({ "type": event.event_type })).or(Err(err))
            }
            _ => Err(err),
        })
    }

    /// Projects this integration event into a CNCF [`CloudEvent`] v1.0 preserving the originating
    /// [`EventEnvelope<A>`]'s deterministic `id` (`{A::TYPE}:{aggregate_id}:{sequence}`),
    /// `timestamp`, and caller provenance (`callerid`, `callertype`).
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialization of `self` into a JSON value fails.
    fn into_projected_cloud_event<A: Aggregate>(
        self,
        envelope: &EventEnvelope<A>,
    ) -> Result<CloudEvent, serde_json::Error>
    where
        Self: Sized,
    {
        let caller_id = envelope.metadata.get("callerid").filter(|s| !s.is_empty()).cloned();
        let caller_type = envelope.metadata.get("callertype").filter(|s| !s.is_empty()).cloned();
        let mut ce = self.into_cloud_event(caller_id, caller_type)?;
        ce.id = format!("{}:{}:{}", A::TYPE, envelope.aggregate_id, envelope.sequence);
        ce.time = envelope
            .metadata
            .get("timestamp")
            .and_then(|timestamp_str| chrono::DateTime::parse_from_rfc3339(timestamp_str).ok())
            .map(|datetime| datetime.with_timezone(&chrono::Utc));
        Ok(ce)
    }
}

/// Implemented by [`Aggregate`]s whose domain events project into public [`IntegrationEvent`]s.
///
/// Aggregates that only emit private internal domain events do not implement this trait.
pub trait ProjectIntegrationEvent: Aggregate {
    /// The public [`IntegrationEvent`] enum that this aggregate's domain events project into.
    type IntegrationEvent: IntegrationEvent;

    /// Projects a domain [`EventEnvelope`] for this aggregate into a public [`CloudEvent`],
    /// returning `None` if the domain event has no public integration event mapping.
    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent>;
}

/// Type-erased descriptor for projecting persisted domain events of a specific [`Aggregate`] into
/// public [`CloudEvent`]s when reading from a persistent store (such as `MongoDB` Change Streams
/// or historical catch-up queries).
#[derive(Clone, Copy)]
pub struct EventProjector {
    aggregate_type: &'static str,
    docs: &'static IntegrationEventGroupDoc,
    project: fn(&str, usize, serde_json::Value, HashMap<String, String>) -> Option<CloudEvent>,
}

impl EventProjector {
    #[must_use]
    pub const fn for_aggregate<A>() -> Self
    where
        A: ProjectIntegrationEvent,
        A::Event: DeserializeOwned,
    {
        Self {
            aggregate_type: A::TYPE,
            docs: &<A::IntegrationEvent as IntegrationEvent>::DOCS,
            project: project_stored_event::<A>,
        }
    }

    #[must_use]
    pub const fn aggregate_type(&self) -> &'static str {
        self.aggregate_type
    }

    #[must_use]
    pub const fn source(&self) -> &'static str {
        self.docs.source
    }

    #[must_use]
    pub const fn docs(&self) -> &'static IntegrationEventGroupDoc {
        self.docs
    }

    #[must_use]
    pub fn project(
        &self,
        aggregate_id: &str,
        sequence: usize,
        payload: serde_json::Value,
        metadata: HashMap<String, String>,
    ) -> Option<CloudEvent> {
        (self.project)(aggregate_id, sequence, payload, metadata)
    }
}

/// Collects the deduplicated [`IntegrationEventGroupDoc`] metadata from a slice of [`EventProjector`]s,
/// preserving the registration order of the first aggregate for each integration event group.
#[must_use]
pub fn collect_projector_docs(projectors: &[EventProjector]) -> Vec<IntegrationEventGroupDoc> {
    let mut groups: Vec<IntegrationEventGroupDoc> = Vec::new();
    for projector in projectors {
        let doc = *projector.docs();
        if !groups.iter().any(|existing| existing.name == doc.name) {
            groups.push(doc);
        }
    }
    groups
}

fn project_stored_event<A>(
    aggregate_id: &str,
    sequence: usize,
    payload: serde_json::Value,
    metadata: HashMap<String, String>,
) -> Option<CloudEvent>
where
    A: ProjectIntegrationEvent,
    A::Event: DeserializeOwned,
{
    let payload: A::Event = serde_json::from_value(payload).ok()?;
    let envelope = EventEnvelope {
        aggregate_id: aggregate_id.to_string(),
        sequence,
        payload,
        metadata,
    };
    A::project_integration_event(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IntegrationEvent;
    use serde::{Deserialize, Serialize};

    /// Sample integration events for testing the derive macro.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, IntegrationEvent)]
    #[serde(tag = "type", content = "data")]
    #[integration_event(source = "/services/sample")]
    enum SampleIntegrationEvent {
        /// Item created in sample service.
        #[serde(rename = "com.impierce.unicore.sample.item.created")]
        ItemCreated {
            /// Unique identifier of the item.
            #[integration_event(subject)]
            item_id: String,
            /// Optional label of the item.
            label: Option<String>,
            /// Boxed metadata to verify `Box<T>` unwrapping in docs.
            metadata: Box<u64>,
        },
    }

    #[test]
    fn derive_generates_trait_and_docs() {
        let event = SampleIntegrationEvent::ItemCreated {
            item_id: "item-1".to_string(),
            label: None,
            metadata: Box::new(42),
        };
        assert_eq!(event.event_type(), "com.impierce.unicore.sample.item.created");
        assert_eq!(event.subject().as_deref(), Some("item-1"));
        assert_eq!(SampleIntegrationEvent::SOURCE, "/services/sample");

        let ce = event.clone().into_cloud_event(None, None).unwrap();
        assert_eq!(ce.event_type, "com.impierce.unicore.sample.item.created");
        assert_eq!(ce.data.as_ref().unwrap()["item_id"], "item-1");
        assert!(ce.data.as_ref().unwrap().get("type").is_none());

        let parsed = SampleIntegrationEvent::from_cloud_event(&ce).unwrap();
        assert_eq!(parsed, event);

        let docs = SampleIntegrationEvent::DOCS;
        assert_eq!(docs.name, "SampleIntegrationEvent");
        assert_eq!(docs.source, "/services/sample");
        assert_eq!(
            docs.description,
            "Sample integration events for testing the derive macro."
        );
        assert_eq!(docs.variants.len(), 1);
        assert_eq!(docs.variants[0].variant_name, "ItemCreated");
        assert_eq!(docs.variants[0].event_type, "com.impierce.unicore.sample.item.created");
        assert_eq!(docs.variants[0].subject_field, Some("item_id"));
        assert_eq!(docs.variants[0].fields.len(), 3);
        assert!(docs.variants[0].fields[0].required);
        assert_eq!(docs.variants[0].fields[0].ty, "String");
        assert!(!docs.variants[0].fields[1].required);
        assert_eq!(docs.variants[0].fields[1].ty, "String");
        assert!(docs.variants[0].fields[2].required);
        assert_eq!(docs.variants[0].fields[2].ty, "u64");

        let md = render_integration_events_markdown("Test Events", &[docs]);
        assert!(md.contains("#### `com.impierce.unicore.sample.item.created`"));
        assert!(md.contains("| `item_id` | `String` | Yes | Unique identifier of the item. |"));
        assert!(md.contains("| `label` | `String` | No | Optional label of the item. |"));
        assert!(md.contains("| `metadata` | `u64` | Yes | Boxed metadata to verify `Box<T>` unwrapping in docs. |"));
    }
}
