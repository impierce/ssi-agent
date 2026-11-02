//! The published event publisher configuration union, discriminated by `publisherType`.
//!
//! `untagged` takes the first branch that parses, so each branch carries its own single-variant
//! `publisherType` marker; a shared enum field would let every payload match the first branch.

use bc_event_publishing::domain::event_publisher::config::{
    EventPublisherConfig, HttpPublisherConfig, NatsPublisherConfig,
};
use serde::{Deserialize, Serialize};
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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpEventPublisherConfigDto {
    pub publisher_type: HttpEventPublisherType,
    #[serde(flatten)]
    pub http: HttpPublisherConfig,
}

/// Configuration for a NATS event publisher.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NatsEventPublisherConfigDto {
    pub publisher_type: NatsEventPublisherType,
    #[serde(flatten)]
    pub nats: NatsPublisherConfig,
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
            EventPublisherConfigDto::Http(config) => EventPublisherConfig::Http(config.http),
            EventPublisherConfigDto::Nats(config) => EventPublisherConfig::Nats(config.nats),
        }
    }
}

impl From<EventPublisherConfig> for EventPublisherConfigDto {
    fn from(config: EventPublisherConfig) -> Self {
        match config {
            EventPublisherConfig::Http(http) => EventPublisherConfigDto::Http(HttpEventPublisherConfigDto {
                publisher_type: HttpEventPublisherType::Http,
                http,
            }),
            EventPublisherConfig::Nats(nats) => EventPublisherConfigDto::Nats(NatsEventPublisherConfigDto {
                publisher_type: NatsEventPublisherType::Nats,
                nats,
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
        EventPublisherConfig, EventPublisherType, HttpPublisherConfig, NatsPublisherConfig,
    };
    use std::collections::BTreeMap;

    fn sample_http_dto() -> EventPublisherConfigDto {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer secret".to_string());
        EventPublisherConfigDto::Http(HttpEventPublisherConfigDto {
            publisher_type: HttpEventPublisherType::Http,
            http: HttpPublisherConfig {
                target_url: "https://subscriber.example.org/events".to_string(),
                headers: Some(headers),
            },
        })
    }

    fn sample_nats_dto() -> EventPublisherConfigDto {
        EventPublisherConfigDto::Nats(NatsEventPublisherConfigDto {
            publisher_type: NatsEventPublisherType::Nats,
            nats: NatsPublisherConfig {
                nats_url: "nats://localhost:4222".to_string(),
                subject: "unicore.events".to_string(),
            },
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
    fn an_unknown_publisher_type_is_rejected() {
        let mut json = serde_json::to_value(sample_http_dto()).unwrap();
        json["publisherType"] = serde_json::Value::String("kafka".to_string());

        assert!(serde_json::from_value::<EventPublisherConfigDto>(json).is_err());
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
