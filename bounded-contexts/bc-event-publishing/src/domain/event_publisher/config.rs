use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

/// Placeholder value returned for HTTP webhook headers in read models and accepted on updates
/// to preserve an existing secret header value without re-transmitting it.
pub const REDACTED_HEADER_VALUE: &str = "****";

/// Identifies the transport mechanism of an [`EventPublisherConfig`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EventPublisherType {
    Http,
    Nats,
}

/// Event subscription filter for an [`EventPublisher`](super::aggregate::EventPublisher).
///
/// Field names match the `/v0/events` SSE query parameters (`types`, and in future extensions
/// `sources`, `subject`, etc.).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventPublisherFilter {
    /// CloudEvent `type` patterns to subscribe to (exact types, prefix globs like
    /// `com.impierce.unicore.issuance.*`, or `*`).
    #[schema(example = json!(["com.impierce.unicore.issuance.credential.offered", "com.impierce.unicore.issuance.credential.issued"]))]
    pub types: Vec<String>,
}

impl EventPublisherFilter {
    /// Converts this publisher filter into an [`EventFilter`](shared_kernel::event_bus::EventFilter)
    /// for subscribing to the event bus.
    #[must_use]
    pub fn to_event_filter(&self) -> shared_kernel::event_bus::EventFilter {
        shared_kernel::event_bus::EventFilter {
            event_types: self.types.clone(),
            ..Default::default()
        }
    }
}

/// Transport-specific configuration for an HTTP webhook event publisher.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpPublisherConfig {
    /// Destination HTTP or HTTPS URL that receives CloudEvents via `POST` with
    /// `Content-Type: application/cloudevents+json`.
    #[serde(alias = "target_url")]
    #[schema(example = "https://example.com/webhooks/unicore")]
    pub target_url: String,
    /// Optional HTTP headers attached to every outgoing webhook request.
    /// Header values are redacted as `"****"` in query responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

/// Transport-specific configuration for a NATS event publisher.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NatsPublisherConfig {
    /// NATS server URL (`nats://host:port` or `tls://host:port`).
    #[serde(alias = "nats_url")]
    #[schema(example = "nats://127.0.0.1:4222")]
    pub nats_url: String,
    /// NATS subject to which matching CloudEvents are published.
    #[schema(example = "unicore.events")]
    pub subject: String,
}

/// Discriminated union of transport configurations for an event publisher.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EventPublisherConfig {
    Http(HttpPublisherConfig),
    Nats(NatsPublisherConfig),
}

/// Why a transport configuration was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum InvalidPublisherConfig {
    #[error("Invalid HTTP target URL `{url}`: {reason}")]
    InvalidTargetUrl { url: String, reason: String },
    #[error("Invalid HTTP header name `{name}`: {reason}")]
    InvalidHeaderName { name: String, reason: String },
    #[error("Invalid HTTP header value for `{name}`: {reason}")]
    InvalidHeaderValue { name: String, reason: String },
    #[error("Invalid NATS URL `{url}`: {reason}")]
    InvalidNatsUrl { url: String, reason: String },
    #[error("NATS subject must not be empty")]
    EmptyNatsSubject,
    #[error("Invalid NATS subject `{subject}`: {reason}")]
    InvalidNatsSubject { subject: String, reason: String },
}

impl HttpPublisherConfig {
    /// Validates and normalizes the HTTP publisher configuration.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPublisherConfig`] if `target_url` is not a valid `http://` or `https://`
    /// URL with a non-empty host, or if any header name or value is syntactically invalid.
    pub fn normalized(self) -> Result<Self, InvalidPublisherConfig> {
        let trimmed_url = self.target_url.trim().to_string();
        let parsed = Url::parse(&trimmed_url).map_err(|err| InvalidPublisherConfig::InvalidTargetUrl {
            url: self.target_url.clone(),
            reason: err.to_string(),
        })?;

        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(InvalidPublisherConfig::InvalidTargetUrl {
                url: self.target_url,
                reason: "scheme must be `http` or `https`".to_string(),
            });
        }

        if parsed.host_str().is_none_or(str::is_empty) {
            return Err(InvalidPublisherConfig::InvalidTargetUrl {
                url: self.target_url,
                reason: "URL must include a non-empty host".to_string(),
            });
        }

        let headers = match self.headers {
            None => None,
            Some(map) if map.is_empty() => None,
            Some(map) => {
                let mut normalized_headers = BTreeMap::new();
                for (name, value) in map {
                    let trimmed_name = name.trim();
                    let header_name = HeaderName::from_bytes(trimmed_name.as_bytes()).map_err(|err| {
                        InvalidPublisherConfig::InvalidHeaderName {
                            name: name.clone(),
                            reason: err.to_string(),
                        }
                    })?;
                    HeaderValue::from_str(&value).map_err(|err| InvalidPublisherConfig::InvalidHeaderValue {
                        name: header_name.as_str().to_string(),
                        reason: err.to_string(),
                    })?;
                    normalized_headers.insert(header_name.as_str().to_string(), value);
                }
                Some(normalized_headers)
            }
        };

        Ok(Self {
            target_url: trimmed_url,
            headers,
        })
    }

    /// Merges preserved header values from `previous` when updating an HTTP publisher:
    /// - If `self.headers` is `None`, preserves `previous.headers`.
    /// - If `self.headers` is `Some({})` (explicitly empty), clears all headers to `None`.
    /// - If an entry has value `"****"` and exists in `previous.headers`, keeps the previous secret value.
    #[must_use]
    pub fn with_preserved_headers(mut self, previous: &HttpPublisherConfig) -> Self {
        match &mut self.headers {
            None => {
                self.headers.clone_from(&previous.headers);
            }
            Some(map) if map.is_empty() => {
                self.headers = None;
            }
            Some(map) => {
                if let Some(prev_headers) = &previous.headers {
                    for (key, value) in map.iter_mut() {
                        if value == REDACTED_HEADER_VALUE {
                            let canonical_key = key.trim().to_ascii_lowercase();
                            if let Some(existing_secret) = prev_headers.get(&canonical_key) {
                                value.clone_from(existing_secret);
                            }
                        }
                    }
                }
            }
        }
        self
    }

    /// Returns a copy of this configuration with all header values replaced by [`REDACTED_HEADER_VALUE`].
    #[must_use]
    pub fn redacted(&self) -> Self {
        Self {
            target_url: self.target_url.clone(),
            headers: self.headers.as_ref().map(|headers| {
                headers
                    .keys()
                    .map(|key| (key.clone(), REDACTED_HEADER_VALUE.to_string()))
                    .collect()
            }),
        }
    }

    /// Converts the configured headers into a [`reqwest::header::HeaderMap`].
    #[must_use]
    pub fn header_map(&self) -> HeaderMap {
        let mut map = HeaderMap::new();
        if let Some(headers) = &self.headers {
            for (name, value) in headers {
                if let (Ok(header_name), Ok(header_value)) =
                    (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value))
                {
                    map.insert(header_name, header_value);
                }
            }
        }
        map
    }
}

impl NatsPublisherConfig {
    /// Validates and normalizes the NATS publisher configuration.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPublisherConfig`] if `nats_url` is not a valid `nats://` or `tls://` URL
    /// with a non-empty host, or if `subject` is empty or contains invalid characters/tokens.
    pub fn normalized(self) -> Result<Self, InvalidPublisherConfig> {
        let trimmed_url = self.nats_url.trim();
        if trimmed_url.is_empty() {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: self.nats_url,
                reason: "NATS URL must not be empty".to_string(),
            });
        }

        let normalized_url = if trimmed_url.contains("://") {
            trimmed_url.to_string()
        } else {
            format!("nats://{trimmed_url}")
        };

        let parsed = Url::parse(&normalized_url).map_err(|err| InvalidPublisherConfig::InvalidNatsUrl {
            url: self.nats_url.clone(),
            reason: err.to_string(),
        })?;

        if parsed.scheme() != "nats" && parsed.scheme() != "tls" {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: self.nats_url,
                reason: "scheme must be `nats` or `tls`".to_string(),
            });
        }

        if parsed.host_str().is_none_or(str::is_empty) {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: self.nats_url,
                reason: "NATS URL must include a non-empty host".to_string(),
            });
        }

        let trimmed_subject = self.subject.trim().to_string();
        if trimmed_subject.is_empty() {
            return Err(InvalidPublisherConfig::EmptyNatsSubject);
        }

        if trimmed_subject.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(InvalidPublisherConfig::InvalidNatsSubject {
                subject: self.subject,
                reason: "subject must not contain whitespace or control characters".to_string(),
            });
        }

        if trimmed_subject.split('.').any(str::is_empty) {
            return Err(InvalidPublisherConfig::InvalidNatsSubject {
                subject: self.subject,
                reason: "subject must not contain empty dot-separated tokens".to_string(),
            });
        }

        Ok(Self {
            nats_url: normalized_url,
            subject: trimmed_subject,
        })
    }
}

impl EventPublisherConfig {
    /// Returns the transport discriminator of this configuration.
    #[must_use]
    pub fn publisher_type(&self) -> EventPublisherType {
        match self {
            Self::Http(_) => EventPublisherType::Http,
            Self::Nats(_) => EventPublisherType::Nats,
        }
    }

    /// Validates and normalizes the configuration.
    ///
    /// # Errors
    ///
    /// Propagates [`InvalidPublisherConfig`] if the underlying transport configuration is invalid.
    pub fn normalized(self) -> Result<Self, InvalidPublisherConfig> {
        Ok(match self {
            Self::Http(http) => Self::Http(http.normalized()?),
            Self::Nats(nats) => Self::Nats(nats.normalized()?),
        })
    }

    /// Merges preserved secret fields (such as HTTP headers) from `previous` when both configurations
    /// are of the same transport type.
    #[must_use]
    pub fn with_preserved_secrets(self, previous: &Self) -> Self {
        match (self, previous) {
            (Self::Http(new_http), Self::Http(prev_http)) => Self::Http(new_http.with_preserved_headers(prev_http)),
            (other, _) => other,
        }
    }

    /// Returns a copy of this configuration with secret values redacted for query responses.
    #[must_use]
    pub fn redacted(&self) -> Self {
        match self {
            Self::Http(http) => Self::Http(http.redacted()),
            Self::Nats(nats) => Self::Nats(nats.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_config_validates_and_normalizes_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_string(), "Bearer secret-token".to_string());
        headers.insert("X-Custom-Header".to_string(), "value".to_string());

        let config = HttpPublisherConfig {
            target_url: " https://example.com/webhook ".to_string(),
            headers: Some(headers),
        }
        .normalized()
        .unwrap();

        assert_eq!(config.target_url, "https://example.com/webhook");
        let normalized_headers = config.headers.as_ref().unwrap();
        assert_eq!(
            normalized_headers.get("authorization").map(String::as_str),
            Some("Bearer secret-token")
        );
        assert_eq!(
            normalized_headers.get("x-custom-header").map(String::as_str),
            Some("value")
        );

        let redacted = config.redacted();
        assert_eq!(
            redacted
                .headers
                .as_ref()
                .unwrap()
                .get("authorization")
                .map(String::as_str),
            Some(REDACTED_HEADER_VALUE)
        );
    }

    #[test]
    fn http_config_rejects_invalid_urls_and_headers() {
        for bad_url in ["not-a-url", "ftp://example.com/webhook", "http://"] {
            let err = HttpPublisherConfig {
                target_url: bad_url.to_string(),
                headers: None,
            }
            .normalized()
            .unwrap_err();
            assert!(matches!(err, InvalidPublisherConfig::InvalidTargetUrl { .. }));
        }

        let mut bad_name = BTreeMap::new();
        bad_name.insert("bad header name".to_string(), "value".to_string());
        assert!(matches!(
            HttpPublisherConfig {
                target_url: "https://example.com/webhook".to_string(),
                headers: Some(bad_name),
            }
            .normalized()
            .unwrap_err(),
            InvalidPublisherConfig::InvalidHeaderName { .. }
        ));
    }

    #[test]
    fn http_config_preserves_redacted_and_omitted_headers_on_update() {
        let mut prev_headers = BTreeMap::new();
        prev_headers.insert("authorization".to_string(), "Bearer top-secret".to_string());
        let previous = HttpPublisherConfig {
            target_url: "https://example.com/v1".to_string(),
            headers: Some(prev_headers),
        };

        // 1. Omitted headers (None) preserves previous headers
        let omitted = HttpPublisherConfig {
            target_url: "https://example.com/v2".to_string(),
            headers: None,
        }
        .with_preserved_headers(&previous);
        assert_eq!(omitted.headers, previous.headers);

        // 2. Redacted placeholder ("****") preserves previous secret for that key
        let mut updated_headers = BTreeMap::new();
        updated_headers.insert("Authorization".to_string(), REDACTED_HEADER_VALUE.to_string());
        updated_headers.insert("x-new".to_string(), "new-val".to_string());
        let merged = HttpPublisherConfig {
            target_url: "https://example.com/v2".to_string(),
            headers: Some(updated_headers),
        }
        .with_preserved_headers(&previous)
        .normalized()
        .unwrap();
        assert_eq!(
            merged
                .headers
                .as_ref()
                .unwrap()
                .get("authorization")
                .map(String::as_str),
            Some("Bearer top-secret")
        );
        assert_eq!(
            merged.headers.as_ref().unwrap().get("x-new").map(String::as_str),
            Some("new-val")
        );

        // 3. Explicit empty map clears headers
        let cleared = HttpPublisherConfig {
            target_url: "https://example.com/v2".to_string(),
            headers: Some(BTreeMap::new()),
        }
        .with_preserved_headers(&previous);
        assert!(cleared.headers.is_none());
    }

    #[test]
    fn nats_config_validates_and_normalizes() {
        let config = NatsPublisherConfig {
            nats_url: "127.0.0.1:4222".to_string(),
            subject: " unicore.issuance.events ".to_string(),
        }
        .normalized()
        .unwrap();

        assert_eq!(config.nats_url, "nats://127.0.0.1:4222");
        assert_eq!(config.subject, "unicore.issuance.events");

        for bad_subject in ["", "   ", "unicore..events", ".leading", "trailing.", "has space"] {
            assert!(NatsPublisherConfig {
                nats_url: "nats://127.0.0.1:4222".to_string(),
                subject: bad_subject.to_string(),
            }
            .normalized()
            .is_err());
        }

        for bad_url in ["", "http://127.0.0.1:4222", "nats://"] {
            assert!(NatsPublisherConfig {
                nats_url: bad_url.to_string(),
                subject: "unicore.events".to_string(),
            }
            .normalized()
            .is_err());
        }
    }
}
