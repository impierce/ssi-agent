//! The published event publisher configuration union, discriminated by `publisherType`.
//!
//! `untagged` takes the first branch that parses, so each branch carries its own single-variant
//! `publisherType` marker; a shared enum field would let every payload match the first branch.

use bc_event_publishing::domain::event_publisher::config::{
    redact_url_for_logging, EventPublisherConfig, HttpPublisherConfig, NatsPublisherConfig, REDACTED_HEADER_VALUE,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;

/// Marks a configuration payload as being for an `http` webhook event publisher.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
pub enum HttpEventPublisherType {
    #[serde(rename = "http")]
    Http,
}

/// Marks a configuration payload as being for a `nats` event publisher.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
pub enum NatsEventPublisherType {
    #[serde(rename = "nats")]
    Nats,
}

/// Configuration for an HTTP webhook event publisher.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpEventPublisherConfigDto {
    pub publisher_type: HttpEventPublisherType,
    /// Destination HTTP or HTTPS URL that receives `CloudEvents` via `POST` with
    /// `Content-Type: application/cloudevents+json`.
    #[schema(min_length = 1, example = "https://example.test/webhooks/unicore")]
    pub target_url: String,
    /// Optional HTTP headers attached to every outgoing webhook request.
    /// Header values are redacted as `"****"` in query responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

impl std::fmt::Debug for HttpEventPublisherConfigDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_headers: Option<BTreeMap<&str, &str>> = self.headers.as_ref().map(|headers| {
            headers
                .keys()
                .map(|key| (key.as_str(), REDACTED_HEADER_VALUE))
                .collect()
        });
        f.debug_struct("HttpEventPublisherConfigDto")
            .field("publisher_type", &self.publisher_type)
            .field("target_url", &redact_url_for_logging(&self.target_url))
            .field("headers", &redacted_headers)
            .finish()
    }
}

/// Configuration for a NATS event publisher.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NatsEventPublisherConfigDto {
    pub publisher_type: NatsEventPublisherType,
    /// NATS server URL (`nats://host:port` or `tls://host:port`).
    #[schema(min_length = 1, example = "nats://127.0.0.1:4222")]
    pub nats_url: String,
    /// NATS subject to which matching `CloudEvents` are published.
    #[schema(min_length = 1, example = "unicore.events")]
    pub subject: String,
}

impl std::fmt::Debug for NatsEventPublisherConfigDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NatsEventPublisherConfigDto")
            .field("publisher_type", &self.publisher_type)
            .field("nats_url", &redact_url_for_logging(&self.nats_url))
            .field("subject", &self.subject)
            .finish()
    }
}

/// Transport-specific configuration for an event publisher, discriminated by `publisherType`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, ToSchema)]
#[serde(untagged)]
#[schema(as = EventPublisherConfig)]
// Without `mapping`, the discriminator value is resolved as a schema name.
#[schema(discriminator(property_name = "publisherType", mapping(
    ("http" = "#/components/schemas/HttpEventPublisherConfigDto"),
    ("nats" = "#/components/schemas/NatsEventPublisherConfigDto"),
)))]
pub enum EventPublisherConfigDto {
    Http(HttpEventPublisherConfigDto),
    Nats(NatsEventPublisherConfigDto),
}

impl From<EventPublisherConfigDto> for EventPublisherConfig {
    fn from(dto: EventPublisherConfigDto) -> Self {
        match dto {
            EventPublisherConfigDto::Http(config) => EventPublisherConfig::Http(HttpPublisherConfig {
                target_url: config.target_url,
                headers: config.headers,
            }),
            EventPublisherConfigDto::Nats(config) => EventPublisherConfig::Nats(NatsPublisherConfig {
                nats_url: config.nats_url,
                subject: config.subject,
            }),
        }
    }
}

impl From<EventPublisherConfig> for EventPublisherConfigDto {
    fn from(config: EventPublisherConfig) -> Self {
        match config {
            EventPublisherConfig::Http(http) => EventPublisherConfigDto::Http(HttpEventPublisherConfigDto {
                publisher_type: HttpEventPublisherType::Http,
                target_url: http.target_url,
                headers: http.headers,
            }),
            EventPublisherConfig::Nats(nats) => EventPublisherConfigDto::Nats(NatsEventPublisherConfigDto {
                publisher_type: NatsEventPublisherType::Nats,
                nats_url: nats.nats_url,
                subject: nats.subject,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EventPublisherConfigDto, HttpEventPublisherConfigDto, HttpEventPublisherType, NatsEventPublisherConfigDto,
        NatsEventPublisherType,
    };
    use bc_event_publishing::domain::event_publisher::config::{
        EventPublisherConfig, EventPublisherType, REDACTED_HEADER_VALUE,
    };
    use std::collections::BTreeMap;

    fn sample_http_dto() -> EventPublisherConfigDto {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer secret".to_string());
        EventPublisherConfigDto::Http(HttpEventPublisherConfigDto {
            publisher_type: HttpEventPublisherType::Http,
            target_url: "https://subscriber.example.org/events".to_string(),
            headers: Some(headers),
        })
    }

    fn sample_nats_dto() -> EventPublisherConfigDto {
        EventPublisherConfigDto::Nats(NatsEventPublisherConfigDto {
            publisher_type: NatsEventPublisherType::Nats,
            nats_url: "nats://localhost:4222".to_string(),
            subject: "unicore.events".to_string(),
        })
    }

    #[test]
    fn the_discriminator_sits_beside_the_transport_fields() {
        let http_json = serde_json::to_value(sample_http_dto()).unwrap();
        assert_eq!(http_json["publisherType"], "http");
        assert_eq!(http_json["targetUrl"], "https://subscriber.example.org/events");
        assert_eq!(http_json["headers"]["authorization"], "Bearer secret");

        let nats_json = serde_json::to_value(sample_nats_dto()).unwrap();
        assert_eq!(nats_json["publisherType"], "nats");
        assert_eq!(nats_json["natsUrl"], "nats://localhost:4222");
        assert_eq!(nats_json["subject"], "unicore.events");
    }

    #[test]
    fn each_publisher_type_deserializes_to_its_own_branch() {
        let http_dto: EventPublisherConfigDto =
            serde_json::from_value(serde_json::to_value(sample_http_dto()).unwrap()).unwrap();
        let http_config: EventPublisherConfig = http_dto.into();
        assert_eq!(http_config.publisher_type(), EventPublisherType::Http);

        let nats_dto: EventPublisherConfigDto =
            serde_json::from_value(serde_json::to_value(sample_nats_dto()).unwrap()).unwrap();
        let nats_config: EventPublisherConfig = nats_dto.into();
        assert_eq!(nats_config.publisher_type(), EventPublisherType::Nats);
    }

    #[test]
    fn unknown_publisher_type_or_unknown_fields_are_rejected() {
        let mut json = serde_json::to_value(sample_http_dto()).unwrap();
        json["publisherType"] = serde_json::Value::String("kafka".to_string());
        assert!(serde_json::from_value::<EventPublisherConfigDto>(json).is_err());

        let mut http_extra = serde_json::to_value(sample_http_dto()).unwrap();
        http_extra["unexpected"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<EventPublisherConfigDto>(http_extra).is_err());

        let mut nats_extra = serde_json::to_value(sample_nats_dto()).unwrap();
        nats_extra["unexpected"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<EventPublisherConfigDto>(nats_extra).is_err());
    }

    #[test]
    fn dto_debug_redacts_secret_headers_capability_paths_and_userinfo_urls() {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_string(), "Bearer top-secret-dto".to_string());
        for secret_url in [
            "https://user:dto-url-secret@example.org/events",
            "https://hooks.example.org/cap/dto-url-secret?sig=dto-query-secret",
        ] {
            let http_dto = EventPublisherConfigDto::Http(HttpEventPublisherConfigDto {
                publisher_type: HttpEventPublisherType::Http,
                target_url: secret_url.to_string(),
                headers: Some(headers.clone()),
            });
            let http_debug = format!("{http_dto:?}");
            assert!(!http_debug.contains("top-secret-dto"));
            assert!(!http_debug.contains("dto-url-secret"));
            assert!(!http_debug.contains("dto-query-secret"));
            assert!(http_debug.contains(REDACTED_HEADER_VALUE));
            assert!(http_debug.contains("<redacted>"));
        }

        let nats_dto = EventPublisherConfigDto::Nats(NatsEventPublisherConfigDto {
            publisher_type: NatsEventPublisherType::Nats,
            nats_url: "nats://user:dto-nats-secret@localhost:4222".to_string(),
            subject: "unicore.events".to_string(),
        });
        let nats_debug = format!("{nats_dto:?}");
        assert!(!nats_debug.contains("dto-nats-secret"));
        assert!(nats_debug.contains("<redacted>"));
    }

    #[test]
    fn the_domain_and_published_shapes_round_trip() {
        for dto in [sample_http_dto(), sample_nats_dto()] {
            let domain: EventPublisherConfig = dto.clone().into();
            let round_tripped = EventPublisherConfigDto::from(domain);
            assert_eq!(round_tripped, dto);
        }
    }
}
