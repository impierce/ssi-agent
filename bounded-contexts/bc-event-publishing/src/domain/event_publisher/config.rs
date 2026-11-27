use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::BTreeMap, net::IpAddr};
use url::{Host, Url};

/// Placeholder value returned for HTTP webhook headers in read models and accepted on updates
/// to preserve an existing secret header value without re-transmitting it.
pub const REDACTED_HEADER_VALUE: &str = "****";

/// Identifies the transport mechanism of an event publisher configuration.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EventPublisherType {
    Http,
    Nats,
}

/// Event subscription filter for an event publisher.
///
/// Field names match the `/v0/events` SSE query parameters (`types`, and in future extensions
/// `sources`, `subject`, etc.).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventPublisherFilter {
    /// `CloudEvent` `type` patterns to subscribe to (exact types, prefix globs like
    /// `com.impierce.unicore.issuance.*`, or `*`).
    #[schema(min_items = 1, example = json!(["com.impierce.unicore.issuance.credential.offered", "com.impierce.unicore.issuance.credential.issued"]))]
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

fn has_scheme_double_slash(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    let mut bytes = scheme.bytes();
    let first = bytes.next()?;
    if !first.is_ascii_alphabetic() || !bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')) {
        return None;
    }
    if rest
        .trim_start_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_control())
        .starts_with(['/', '\\'])
    {
        return None;
    }
    Some(rest)
}

fn authority_contains_at(url: &str) -> bool {
    let Some(after_scheme) = has_scheme_double_slash(url) else {
        return url.contains('@');
    };
    let authority_end = after_scheme.find(['/', '\\', '?', '#']).unwrap_or(after_scheme.len());
    after_scheme[..authority_end].contains('@')
}

fn parse_http_url(raw_url: &str) -> Option<Url> {
    let trimmed = raw_url.trim();
    if trimmed.bytes().any(|b| b.is_ascii_control())
        || has_scheme_double_slash(trimmed).is_none()
        || authority_contains_at(trimmed)
    {
        return None;
    }
    let parsed = Url::parse(trimmed).ok()?;
    if matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|h| !h.trim_end_matches('.').is_empty())
    {
        Some(parsed)
    } else {
        None
    }
}

/// Whether `ip` is a globally routable public address that cannot reach the host, private network,
/// or cloud metadata endpoints.
///
/// Hand-rolled because `IpAddr::is_global` is still unstable.
pub(crate) fn is_globally_routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, ..] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_multicast()
                || ip.is_unspecified()
                // 0.0.0.0/8 (current network)
                || a == 0
                // 100.64.0.0/10 (carrier-grade NAT)
                || (a == 100 && (b & 0xc0) == 0x40)
                // 192.0.0.0/24 (IETF protocol assignments)
                || (a == 192 && b == 0 && c == 0)
                // 198.18.0.0/15 (benchmarking, RFC 2544)
                || (a == 198 && (b & 0xfe) == 18)
                // 240.0.0.0/4 (reserved)
                || a >= 240)
        }
        IpAddr::V6(ip) => {
            if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
                return false;
            }
            if let Some(ipv4) = ip.to_ipv4() {
                return is_globally_routable(IpAddr::V4(ipv4));
            }
            let segments = ip.segments();
            let first = segments[0];
            // Only IANA Global Unicast (`2000::/3`) is globally routable; everything else
            // (including `::/8`, SIIT `::ffff:0:0/96`, NAT64 `64:ff9b::/32`, discard `100::/64`,
            // ULA `fc00::/7`, link-local `fe80::/10`, site-local `fec0::/10`, multicast `ff00::/8`,
            // and unallocated IETF space) is non-global.
            if (first & 0xe000) != 0x2000 {
                return false;
            }
            // Exclude non-global, transition, documentation, and benchmarking sub-allocations
            // within `2000::/3`:
            // - 2001::/32 (Teredo)
            // - 2001:2::/48 (benchmarking, RFC 5180)
            // - 2001:10::/28 (ORCHID, RFC 4843)
            // - 2001:db8::/32 (documentation, RFC 3849)
            // - 2002::/16 (6to4, RFC 3056 / deprecated RFC 7526 — can embed private/loopback IPv4)
            !(first == 0x2002
                || (first == 0x2001
                    && (segments[1] == 0x0000
                        || (segments[1] == 0x0002 && segments[2] == 0x0000)
                        || (segments[1] & 0xfff0) == 0x0010
                        || segments[1] == 0x0db8)))
        }
    }
}

/// Whether `ip` is a loopback or private local-network address permitted only when `allow-localhost`
/// (or test-only local-network opt-in) is enabled. Link-local and other reserved ranges stay blocked.
pub(crate) fn is_local_network(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => {
            if ip.is_loopback() {
                return true;
            }
            if let Some(ipv4) = ip.to_ipv4() {
                return is_local_network(IpAddr::V4(ipv4));
            }
            let first = ip.segments()[0];
            (first & 0xfe00) == 0xfc00
        }
    }
}

pub(crate) fn is_ip_allowed(ip: IpAddr, allow_local_network: bool) -> bool {
    is_globally_routable(ip) || (allow_local_network && is_local_network(ip))
}

fn is_local_domain(domain: &str) -> bool {
    let normalized = domain.trim_end_matches('.');
    normalized.eq_ignore_ascii_case("localhost")
        || normalized.rsplit_once('.').is_some_and(|(_, tld)| {
            tld.eq_ignore_ascii_case("localhost")
                || tld.eq_ignore_ascii_case("local")
                || tld.eq_ignore_ascii_case("internal")
        })
}

pub(crate) fn validate_http_target_url(url: &Url, allow_local_network: bool) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("scheme must be `http` or `https`".to_string());
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err("HTTP target URL must not contain embedded userinfo credentials".to_string());
    }

    match url.host() {
        None => Err("URL must include a non-empty host".to_string()),
        Some(Host::Ipv4(ip)) => {
            if is_ip_allowed(IpAddr::V4(ip), allow_local_network) {
                Ok(())
            } else {
                Err("HTTP target URL host is not a permitted destination address".to_string())
            }
        }
        Some(Host::Ipv6(ip)) => {
            if is_ip_allowed(IpAddr::V6(ip), allow_local_network) {
                Ok(())
            } else {
                Err("HTTP target URL host is not a permitted destination address".to_string())
            }
        }
        Some(Host::Domain(domain)) => {
            let normalized = domain.trim_end_matches('.');
            if normalized.is_empty() {
                return Err("URL must include a non-empty host".to_string());
            }
            if normalized.eq_ignore_ascii_case("metadata.google.internal")
                || normalized.eq_ignore_ascii_case("metadata.goog")
                || (!allow_local_network && is_local_domain(normalized))
            {
                return Err("HTTP target URL host is not a permitted destination".to_string());
            }
            Ok(())
        }
    }
}

/// Redacts sensitive components of `raw_url` before formatting in `Debug` output or validation
/// error messages:
/// - Returns `""` when `raw_url` is empty or whitespace.
/// - Returns `"<redacted>"` whenever `raw_url` contains `@` (covering embedded userinfo credentials
///   as well as non-hierarchical `mailto:` / `urn:` or malformed inputs), fails to parse as a
///   hierarchical URL (other than a bare `host:port` broker address), or lacks a host.
/// - Replaces non-root paths, query strings, and fragments with `/<redacted>` so capability webhook
///   paths and query secrets never leak in logs or error strings.
#[must_use]
pub fn redact_url_for_logging(raw_url: &str) -> Cow<'_, str> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Cow::Borrowed("");
    }
    if raw_url.contains('@') {
        return Cow::Borrowed("<redacted>");
    }

    if has_scheme_double_slash(trimmed).is_some() {
        if let Ok(parsed) = Url::parse(trimmed) {
            let Some(host) = parsed.host_str().filter(|h| !h.is_empty()) else {
                return Cow::Borrowed("<redacted>");
            };
            let origin = match parsed.port() {
                Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
                None => format!("{}://{host}", parsed.scheme()),
            };
            let has_sensitive_suffix = (!parsed.path().is_empty() && parsed.path() != "/")
                || parsed.query().is_some()
                || parsed.fragment().is_some();
            return Cow::Owned(if has_sensitive_suffix {
                format!("{origin}/<redacted>")
            } else {
                origin
            });
        }
    }

    if !trimmed.contains("://") {
        if let Some((host_part, port_part)) = trimmed.rsplit_once(':') {
            if !host_part.is_empty() && port_part.parse::<u16>().is_ok() {
                if let Ok(parsed) = Url::parse(&format!("nats://{trimmed}")) {
                    if let (Some(host), Some(port)) = (parsed.host_str().filter(|h| !h.is_empty()), parsed.port()) {
                        if parsed.path().is_empty() && parsed.query().is_none() && parsed.fragment().is_none() {
                            return Cow::Owned(format!("{host}:{port}"));
                        }
                    }
                }
            }
        }
    }

    Cow::Borrowed("<redacted>")
}

/// Transport-specific configuration for an HTTP webhook event publisher.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpPublisherConfig {
    /// Destination HTTP or HTTPS URL that receives `CloudEvents` via `POST` with
    /// `Content-Type: application/cloudevents+json`.
    pub target_url: String,
    /// Optional HTTP headers attached to every outgoing webhook request.
    /// Header values are redacted as `"****"` in query responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

impl std::fmt::Debug for HttpPublisherConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_headers: Option<BTreeMap<&str, &str>> = self.headers.as_ref().map(|headers| {
            headers
                .keys()
                .map(|key| (key.as_str(), REDACTED_HEADER_VALUE))
                .collect()
        });
        f.debug_struct("HttpPublisherConfig")
            .field("target_url", &redact_url_for_logging(&self.target_url))
            .field("headers", &redacted_headers)
            .finish()
    }
}

/// Transport-specific configuration for a NATS event publisher.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NatsPublisherConfig {
    /// NATS server URL (`nats://host:port` or `tls://host:port`).
    pub nats_url: String,
    /// NATS subject to which matching `CloudEvents` are published.
    pub subject: String,
}

impl std::fmt::Debug for NatsPublisherConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NatsPublisherConfig")
            .field("nats_url", &redact_url_for_logging(&self.nats_url))
            .field("subject", &self.subject)
            .finish()
    }
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
    #[error(
        "Cannot preserve existing HTTP headers when changing target URL origin; provide explicit `headers` or `{{}}` to clear them"
    )]
    CrossOriginHeaderPreservationNotAllowed,
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
    /// URL with a permitted destination host, or if any header name is reserved/invalid or header
    /// value is invalid.
    pub fn normalized(self) -> Result<Self, InvalidPublisherConfig> {
        self.normalized_with_local_network(cfg!(feature = "allow-localhost"))
    }

    pub(crate) fn normalized_with_local_network(
        self,
        allow_local_network: bool,
    ) -> Result<Self, InvalidPublisherConfig> {
        let trimmed_url = self.target_url.trim().to_string();
        if trimmed_url.bytes().any(|b| b.is_ascii_control()) {
            return Err(InvalidPublisherConfig::InvalidTargetUrl {
                url: redact_url_for_logging(&self.target_url).into_owned(),
                reason: "HTTP target URL must not contain control characters".to_string(),
            });
        }

        let parsed = Url::parse(&trimmed_url).map_err(|err| InvalidPublisherConfig::InvalidTargetUrl {
            url: redact_url_for_logging(&self.target_url).into_owned(),
            reason: err.to_string(),
        })?;

        if !parsed.username().is_empty() || parsed.password().is_some() || authority_contains_at(&trimmed_url) {
            return Err(InvalidPublisherConfig::InvalidTargetUrl {
                url: "<redacted>".to_string(),
                reason: "HTTP target URL must not contain embedded userinfo credentials".to_string(),
            });
        }

        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(InvalidPublisherConfig::InvalidTargetUrl {
                url: redact_url_for_logging(&self.target_url).into_owned(),
                reason: "scheme must be `http` or `https`".to_string(),
            });
        }

        if has_scheme_double_slash(&trimmed_url).is_none() {
            return Err(InvalidPublisherConfig::InvalidTargetUrl {
                url: redact_url_for_logging(&self.target_url).into_owned(),
                reason: "URL must start with `http://` or `https://`".to_string(),
            });
        }

        if let Err(reason) = validate_http_target_url(&parsed, allow_local_network) {
            return Err(InvalidPublisherConfig::InvalidTargetUrl {
                url: redact_url_for_logging(&self.target_url).into_owned(),
                reason,
            });
        }

        let headers = normalize_http_headers(self.headers)?;

        Ok(Self {
            target_url: trimmed_url,
            headers,
        })
    }

    /// Merges preserved header values from `previous` when updating an HTTP publisher:
    /// - When `self.target_url` and `previous.target_url` share the same HTTP/HTTPS origin
    ///   (scheme, host, and effective port):
    ///   - `None` preserves `previous.headers`.
    ///   - `Some({})` clears all headers to `None`.
    ///   - Entries with value `"****"` keep the existing secret from `previous.headers`.
    /// - When valid HTTP/HTTPS origins differ and `previous.headers` is non-empty, omitted
    ///   `headers` (`None`) and `"****"` placeholders for existing headers are rejected so secrets
    ///   cannot be silently forwarded to a different origin; callers must supply explicit
    ///   replacement headers or `{}` to clear them.
    /// - When `self.target_url` is malformed or non-HTTP/HTTPS, returns `Ok(self)` without
    ///   preserving secrets so subsequent [`Self::normalized`] reports [`InvalidPublisherConfig::InvalidTargetUrl`].
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPublisherConfig::CrossOriginHeaderPreservationNotAllowed`] if changing to a
    /// different HTTP/HTTPS origin while trying to preserve existing headers via omission or `"****"`.
    pub fn with_preserved_headers(mut self, previous: &HttpPublisherConfig) -> Result<Self, InvalidPublisherConfig> {
        let Some(new_url) = parse_http_url(&self.target_url) else {
            return Ok(self);
        };

        let same_origin = parse_http_url(&previous.target_url).is_some_and(|prev| prev.origin() == new_url.origin());

        if same_origin {
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
                                if let Some(existing_secret) = find_previous_header(prev_headers, key) {
                                    existing_secret.clone_into(value);
                                }
                            }
                        }
                    }
                }
            }
            return Ok(self);
        }

        if let Some(prev_headers) = previous.headers.as_ref().filter(|headers| !headers.is_empty()) {
            match &self.headers {
                None => return Err(InvalidPublisherConfig::CrossOriginHeaderPreservationNotAllowed),
                Some(map) if map.is_empty() => self.headers = None,
                Some(map) => {
                    for (key, value) in map {
                        if value == REDACTED_HEADER_VALUE && find_previous_header(prev_headers, key).is_some() {
                            return Err(InvalidPublisherConfig::CrossOriginHeaderPreservationNotAllowed);
                        }
                    }
                }
            }
        } else if matches!(&self.headers, Some(map) if map.is_empty()) {
            self.headers = None;
        }

        Ok(self)
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

fn find_previous_header<'a>(prev_headers: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    let trimmed_key = key.trim();
    prev_headers
        .get(&trimmed_key.to_ascii_lowercase())
        .or_else(|| {
            prev_headers
                .iter()
                .find(|(existing_key, _)| existing_key.trim().eq_ignore_ascii_case(trimmed_key))
                .map(|(_, value)| value)
        })
        .map(String::as_str)
}

fn normalize_http_headers(
    headers: Option<BTreeMap<String, String>>,
) -> Result<Option<BTreeMap<String, String>>, InvalidPublisherConfig> {
    match headers {
        None => Ok(None),
        Some(map) if map.is_empty() => Ok(None),
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
                if matches!(header_name, CONTENT_TYPE | CONTENT_LENGTH | HOST) {
                    return Err(InvalidPublisherConfig::InvalidHeaderName {
                        name,
                        reason: format!("`{header_name}` is a reserved HTTP header managed by the publisher"),
                    });
                }
                if normalized_headers.contains_key(header_name.as_str()) {
                    return Err(InvalidPublisherConfig::InvalidHeaderName {
                        name,
                        reason: "duplicate HTTP header name (header names are case-insensitive)".to_string(),
                    });
                }
                if value == REDACTED_HEADER_VALUE {
                    return Err(InvalidPublisherConfig::InvalidHeaderValue {
                        name: header_name.as_str().to_string(),
                        reason: "header value cannot be the redacted placeholder `****` when no existing header value is present to preserve".to_string(),
                    });
                }
                HeaderValue::from_str(&value).map_err(|err| InvalidPublisherConfig::InvalidHeaderValue {
                    name: header_name.as_str().to_string(),
                    reason: err.to_string(),
                })?;
                normalized_headers.insert(header_name.as_str().to_string(), value);
            }
            Ok(Some(normalized_headers))
        }
    }
}

impl NatsPublisherConfig {
    /// Validates and normalizes the NATS publisher configuration.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPublisherConfig`] if `nats_url` is not a valid `nats://` or `tls://` URL
    /// with a non-empty host and no path/query/fragment, or if `subject` is empty, contains
    /// wildcards (`*` / `>`), or contains invalid characters/tokens.
    pub fn normalized(self) -> Result<Self, InvalidPublisherConfig> {
        let trimmed_url = self.nats_url.trim();
        if trimmed_url.is_empty() {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: redact_url_for_logging(&self.nats_url).into_owned(),
                reason: "NATS URL must not be empty".to_string(),
            });
        }
        if trimmed_url.bytes().any(|b| b.is_ascii_control()) {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: redact_url_for_logging(&self.nats_url).into_owned(),
                reason: "NATS URL must not contain control characters".to_string(),
            });
        }

        let candidate_url = if trimmed_url.contains("://") {
            trimmed_url.to_string()
        } else {
            format!("nats://{trimmed_url}")
        };

        let mut parsed = Url::parse(&candidate_url).map_err(|err| InvalidPublisherConfig::InvalidNatsUrl {
            url: redact_url_for_logging(&self.nats_url).into_owned(),
            reason: err.to_string(),
        })?;

        if !parsed.username().is_empty() || parsed.password().is_some() || authority_contains_at(&candidate_url) {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: "<redacted>".to_string(),
                reason: "NATS URL must not contain embedded userinfo credentials".to_string(),
            });
        }

        if parsed.scheme() != "nats" && parsed.scheme() != "tls" {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: redact_url_for_logging(&self.nats_url).into_owned(),
                reason: "scheme must be `nats` or `tls`".to_string(),
            });
        }

        if has_scheme_double_slash(&candidate_url).is_none() {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: redact_url_for_logging(&self.nats_url).into_owned(),
                reason: "NATS URL must start with `nats://` or `tls://`".to_string(),
            });
        }

        if parsed.host_str().is_none_or(str::is_empty) {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: redact_url_for_logging(&self.nats_url).into_owned(),
                reason: "NATS URL must include a non-empty host".to_string(),
            });
        }

        if (!parsed.path().is_empty() && parsed.path() != "/")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(InvalidPublisherConfig::InvalidNatsUrl {
                url: redact_url_for_logging(&self.nats_url).into_owned(),
                reason: "NATS URL must not contain a path, query, or fragment".to_string(),
            });
        }
        if parsed.path() == "/" {
            parsed.set_path("");
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

        if trimmed_subject.contains(['*', '>']) {
            return Err(InvalidPublisherConfig::InvalidNatsSubject {
                subject: self.subject,
                reason: "subject must not contain NATS wildcard characters (`*` or `>`)".to_string(),
            });
        }

        if trimmed_subject.split('.').any(str::is_empty) {
            return Err(InvalidPublisherConfig::InvalidNatsSubject {
                subject: self.subject,
                reason: "subject must not contain empty dot-separated tokens".to_string(),
            });
        }

        Ok(Self {
            nats_url: parsed.to_string(),
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
        self.normalized_with_local_network(cfg!(feature = "allow-localhost"))
    }

    pub(crate) fn normalized_with_local_network(
        self,
        allow_local_network: bool,
    ) -> Result<Self, InvalidPublisherConfig> {
        Ok(match self {
            Self::Http(http) => Self::Http(http.normalized_with_local_network(allow_local_network)?),
            Self::Nats(nats) => Self::Nats(nats.normalized()?),
        })
    }

    /// Merges preserved secret fields (such as HTTP headers) from `previous` when both configurations
    /// are of the same transport type.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPublisherConfig::CrossOriginHeaderPreservationNotAllowed`] when an HTTP
    /// update changes the target URL origin without replacing or explicitly clearing existing headers.
    pub fn with_preserved_secrets(self, previous: &Self) -> Result<Self, InvalidPublisherConfig> {
        match (self, previous) {
            (Self::Http(new_http), Self::Http(prev_http)) => {
                Ok(Self::Http(new_http.with_preserved_headers(prev_http)?))
            }
            (other, _) => Ok(other),
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
            target_url: " https://example.test/webhook ".to_string(),
            headers: Some(headers),
        }
        .normalized()
        .unwrap();

        assert_eq!(config.target_url, "https://example.test/webhook");
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

        // `@` inside the path (rather than authority) is valid for HTTP webhooks, while its Debug
        // representation still redacts it.
        let at_in_path = HttpPublisherConfig {
            target_url: "https://example.test/@org/webhook".to_string(),
            headers: None,
        }
        .normalized()
        .unwrap();
        assert_eq!(at_in_path.target_url, "https://example.test/@org/webhook");
        assert!(!format!("{at_in_path:?}").contains("@org"));
    }

    #[test]
    fn http_config_rejects_invalid_urls_and_headers_without_leaking_secrets() {
        for bad_url in [
            "not-a-url",
            "sk_live_secret-token",
            "https:example.test/secret-path",
            "hooks.example.test/services/secret-path",
            "htps://hooks.example.test/cap/secret-path?token=secret-token",
            "https://hooks.example.test:99999/cap/secret-path?token=secret-token",
            "ftp://example.test/secret-path?token=secret-token",
            "http://",
            "https://@example.test/webhook",
            "https://:@example.test/webhook",
            "https://user:password@example.test/webhook",
            "https://secret-token@example.test/webhook",
            "mailto:secret-token@example.test",
            "urn:unicore:secret-token@example.test",
            "not a url with secret-token@example.test",
        ] {
            let err = HttpPublisherConfig {
                target_url: bad_url.to_string(),
                headers: None,
            }
            .normalized()
            .unwrap_err();
            assert!(
                matches!(err, InvalidPublisherConfig::InvalidTargetUrl { .. }),
                "expected InvalidTargetUrl for `{bad_url}`, got {err:?}"
            );
            let rendered = err.to_string();
            assert!(!rendered.contains("password"), "leaked password in `{rendered}`");
            assert!(
                !rendered.contains("secret-token"),
                "leaked secret-token in `{rendered}`"
            );
            assert!(!rendered.contains("secret-path"), "leaked secret-path in `{rendered}`");
        }

        let mut bad_name = BTreeMap::new();
        bad_name.insert("bad header name".to_string(), "value".to_string());
        assert!(matches!(
            HttpPublisherConfig {
                target_url: "https://example.test/webhook".to_string(),
                headers: Some(bad_name),
            }
            .normalized()
            .unwrap_err(),
            InvalidPublisherConfig::InvalidHeaderName { .. }
        ));

        for reserved in [
            "Content-Type",
            "content-type",
            " cOnTeNt-TyPe ",
            "Content-Length",
            " content-length ",
            "HOST",
            "host",
            " hOsT ",
        ] {
            let mut reserved_headers = BTreeMap::new();
            reserved_headers.insert(reserved.to_string(), "custom".to_string());
            assert!(matches!(
                HttpPublisherConfig {
                    target_url: "https://example.test/webhook".to_string(),
                    headers: Some(reserved_headers),
                }
                .normalized()
                .unwrap_err(),
                InvalidPublisherConfig::InvalidHeaderName { .. }
            ));
        }

        let mut duplicate_headers = BTreeMap::new();
        duplicate_headers.insert("Authorization".to_string(), "Bearer first".to_string());
        duplicate_headers.insert("authorization".to_string(), "Bearer second".to_string());
        assert!(matches!(
            HttpPublisherConfig {
                target_url: "https://example.test/webhook".to_string(),
                headers: Some(duplicate_headers),
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
            target_url: "https://example.test/v1".to_string(),
            headers: Some(prev_headers),
        };

        // 1. Omitted headers (None) preserves previous headers
        let omitted = HttpPublisherConfig {
            target_url: "https://example.test/v2".to_string(),
            headers: None,
        }
        .with_preserved_headers(&previous)
        .unwrap();
        assert_eq!(omitted.headers, previous.headers);

        // 2. Redacted placeholder ("****") preserves previous secret for that key
        let mut updated_headers = BTreeMap::new();
        updated_headers.insert("Authorization".to_string(), REDACTED_HEADER_VALUE.to_string());
        updated_headers.insert("x-new".to_string(), "new-val".to_string());
        let merged = HttpPublisherConfig {
            target_url: "https://example.test:443/v2".to_string(),
            headers: Some(updated_headers),
        }
        .with_preserved_headers(&previous)
        .unwrap()
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
            target_url: "https://example.test/v2".to_string(),
            headers: Some(BTreeMap::new()),
        }
        .with_preserved_headers(&previous)
        .unwrap();
        assert!(cleared.headers.is_none());
    }

    #[test]
    fn http_config_rejects_cross_origin_header_preservation_and_allows_explicit_or_clear() {
        let mut prev_headers = BTreeMap::new();
        prev_headers.insert("authorization".to_string(), "Bearer top-secret".to_string());
        let previous = HttpPublisherConfig {
            target_url: "https://trusted.example.test/v1".to_string(),
            headers: Some(prev_headers),
        };

        // Case-insensitive host & default port still match the same origin
        let same_origin = HttpPublisherConfig {
            target_url: "https://TRUSTED.EXAMPLE.TEST:443/v2".to_string(),
            headers: None,
        }
        .with_preserved_headers(&previous)
        .unwrap();
        assert_eq!(same_origin.headers, previous.headers);

        // Different host, port, or scheme must reject omitted headers and "****" placeholders
        for other_origin in [
            "https://other.example.test/v1",
            "https://trusted.example.test:8443/v1",
            "http://trusted.example.test/v1",
        ] {
            let omitted_err = HttpPublisherConfig {
                target_url: other_origin.to_string(),
                headers: None,
            }
            .with_preserved_headers(&previous)
            .unwrap_err();
            assert_eq!(
                omitted_err,
                InvalidPublisherConfig::CrossOriginHeaderPreservationNotAllowed
            );

            let mut placeholder = BTreeMap::new();
            placeholder.insert("Authorization".to_string(), REDACTED_HEADER_VALUE.to_string());
            let placeholder_err = HttpPublisherConfig {
                target_url: other_origin.to_string(),
                headers: Some(placeholder),
            }
            .with_preserved_headers(&previous)
            .unwrap_err();
            assert_eq!(
                placeholder_err,
                InvalidPublisherConfig::CrossOriginHeaderPreservationNotAllowed
            );

            // Explicit empty map clears headers on cross-origin move
            let cleared = HttpPublisherConfig {
                target_url: other_origin.to_string(),
                headers: Some(BTreeMap::new()),
            }
            .with_preserved_headers(&previous)
            .unwrap()
            .normalized()
            .unwrap();
            assert!(cleared.headers.is_none());

            // Explicit replacement headers succeed on cross-origin move
            let mut replacement = BTreeMap::new();
            replacement.insert("Authorization".to_string(), "Bearer new-secret".to_string());
            let replaced = HttpPublisherConfig {
                target_url: other_origin.to_string(),
                headers: Some(replacement),
            }
            .with_preserved_headers(&previous)
            .unwrap()
            .normalized()
            .unwrap();
            assert_eq!(
                replaced
                    .headers
                    .as_ref()
                    .unwrap()
                    .get("authorization")
                    .map(String::as_str),
                Some("Bearer new-secret")
            );
        }

        // Cross-origin move when previous had no headers allows omitted headers
        let previous_without_headers = HttpPublisherConfig {
            target_url: "https://trusted.example.test/v1".to_string(),
            headers: None,
        };
        let moved_without_headers = HttpPublisherConfig {
            target_url: "https://other.example.test/v1".to_string(),
            headers: None,
        }
        .with_preserved_headers(&previous_without_headers)
        .unwrap()
        .normalized()
        .unwrap();
        assert!(moved_without_headers.headers.is_none());
    }

    #[test]
    fn http_config_header_preservation_matches_unnormalized_previous_keys_case_insensitively() {
        let mut mixed_case_prev = BTreeMap::new();
        mixed_case_prev.insert("Authorization".to_string(), "Bearer mixed-secret".to_string());
        let unnormalized_previous = HttpPublisherConfig {
            target_url: "https://trusted.example.test/v1".to_string(),
            headers: Some(mixed_case_prev),
        };
        let mut lower_placeholder = BTreeMap::new();
        lower_placeholder.insert("authorization".to_string(), REDACTED_HEADER_VALUE.to_string());

        let same_origin_mixed = HttpPublisherConfig {
            target_url: "https://trusted.example.test/v2".to_string(),
            headers: Some(lower_placeholder.clone()),
        }
        .with_preserved_headers(&unnormalized_previous)
        .unwrap();
        assert_eq!(
            same_origin_mixed
                .headers
                .as_ref()
                .unwrap()
                .get("authorization")
                .map(String::as_str),
            Some("Bearer mixed-secret")
        );

        let cross_origin_mixed_err = HttpPublisherConfig {
            target_url: "https://other.example.test/v1".to_string(),
            headers: Some(lower_placeholder),
        }
        .with_preserved_headers(&unnormalized_previous)
        .unwrap_err();
        assert_eq!(
            cross_origin_mixed_err,
            InvalidPublisherConfig::CrossOriginHeaderPreservationNotAllowed
        );
    }

    #[test]
    fn http_config_invalid_url_on_update_still_reports_invalid_target_url() {
        let mut prev_headers = BTreeMap::new();
        prev_headers.insert("authorization".to_string(), "Bearer top-secret".to_string());
        let previous = HttpPublisherConfig {
            target_url: "https://trusted.example.test/v1".to_string(),
            headers: Some(prev_headers),
        };

        for invalid_url in [
            "ftp://other.example.test/v1",
            "not-a-url",
            "https://user:pass@other.example.test/v1",
            "https:other.example.test/foo://bar",
            "https://./v1",
            "https://.../v1",
            "https://ex\tample.test/v1",
            "https://other.example.test/v1\r\nX-Injected: 1",
        ] {
            let err = HttpPublisherConfig {
                target_url: invalid_url.to_string(),
                headers: None,
            }
            .with_preserved_headers(&previous)
            .unwrap()
            .normalized()
            .unwrap_err();
            assert!(
                matches!(err, InvalidPublisherConfig::InvalidTargetUrl { .. }),
                "expected InvalidTargetUrl for `{invalid_url}`, got {err:?}"
            );
        }
    }

    #[test]
    fn scheme_double_slash_and_dot_segment_redaction_are_strict() {
        // Valid URL with `://` inside query/fragment is allowed
        let with_query_scheme = HttpPublisherConfig {
            target_url: "https://example.test/webhook?next=https://other.test#ref://x".to_string(),
            headers: None,
        }
        .normalized()
        .unwrap();
        assert_eq!(
            with_query_scheme.target_url,
            "https://example.test/webhook?next=https://other.test#ref://x"
        );

        // Non-scheme `://`, extra leading slashes, backslashes after `://`, and embedded control
        // characters must be rejected without leaking secrets.
        for bad_url in [
            "https:example.test/foo://bar",
            "https:@example.test/foo://bar",
            "https:///example.test/secret-token",
            "https://\\example.test/secret-token",
            "https://\t/example.test/secret-token",
            "https://ex\tample.test/secret-token",
            "https://example.test/secret-token\r\n/next",
            "1https://example.test/hook",
        ] {
            let err = HttpPublisherConfig {
                target_url: bad_url.to_string(),
                headers: None,
            }
            .normalized()
            .unwrap_err();
            assert!(
                matches!(err, InvalidPublisherConfig::InvalidTargetUrl { .. }),
                "expected InvalidTargetUrl for `{bad_url}`, got {err:?}"
            );
            assert!(!err.to_string().contains("secret-token"));
        }

        // Dot-segment paths collapsing to `/` must never leak the raw path into logs or errors
        for dot_url in [
            "https://example.test/secret-token/..",
            "https://example.test:8443/a/secret-token/../..",
            "https://example.test/secret-token/../",
            "ftp://example.test/secret-token/..",
        ] {
            let redacted = redact_url_for_logging(dot_url);
            assert!(
                !redacted.contains("secret-token"),
                "leaked dot-segment secret for `{dot_url}` in `{redacted}`"
            );
        }
        assert_eq!(
            redact_url_for_logging("https://example.test/secret-token/.."),
            "https://example.test"
        );
        assert_eq!(
            redact_url_for_logging("https://example.test:8443/a/secret-token/../.."),
            "https://example.test:8443"
        );
    }

    #[test]
    fn http_config_blocks_non_global_destinations_and_allows_public_http_and_https() {
        // Plaintext HTTP and HTTPS for public domains and public IPs are permitted
        for allowed in [
            "http://subscriber.example.org/webhooks/unicore",
            "https://subscriber.example.org/events",
            "http://93.184.216.34/webhook",
            "https://[2606:2800:220:1:248:1893:25c8:1946]/webhook",
            "https://[::ffff:93.184.216.34]/webhook",
        ] {
            assert!(
                HttpPublisherConfig {
                    target_url: allowed.to_string(),
                    headers: None,
                }
                .normalized_with_local_network(false)
                .is_ok(),
                "expected `{allowed}` to be allowed"
            );
        }

        // Non-global IP literals and local domains are rejected when local network is disallowed
        for blocked in [
            "http://127.0.0.1/secret-path",
            "http://127.0.0.2:8080/secret-path",
            "http://[::1]:8080/secret-path",
            "http://2130706433/secret-path",
            "http://0x7f000001/secret-path",
            "http://[::ffff:127.0.0.1]/secret-path",
            "http://[::ffff:0:127.0.0.1]/secret-path",
            "http://[::ffff:0:169.254.169.254]/secret-path",
            "http://[2002:7f00:1::]/secret-path",
            "http://[2002:a9fe:a9fe::]/secret-path",
            "http://[100::1]/secret-path",
            "http://[2001::1]/secret-path",
            "http://[2001:2::1]/secret-path",
            "http://[2001:10::1]/secret-path",
            "http://[4000::1]/secret-path",
            "http://[fe00::1]/secret-path",
            "http://localhost:8080/secret-path",
            "http://LOCALHOST./secret-path",
            "http://app.localhost/secret-path",
            "http://10.0.0.1/secret-path",
            "http://172.16.0.1/secret-path",
            "http://192.168.1.1/secret-path",
            "http://[fd00::1]/secret-path",
            "http://169.254.169.254/latest/meta-data/secret-path",
            "http://[::ffff:169.254.169.254]/secret-path",
            "http://[fe80::1]/secret-path",
            "http://0.0.0.0/secret-path",
            "http://[::]/secret-path",
            "http://100.64.0.1/secret-path",
            "http://224.0.0.1/secret-path",
            "http://[ff02::1]/secret-path",
            "http://255.255.255.255/secret-path",
            "http://240.0.0.1/secret-path",
            "http://192.0.2.1/secret-path",
            "http://198.18.0.1/secret-path",
            "http://[2001:db8::1]/secret-path",
            "http://[64:ff9b::127.0.0.1]/secret-path",
            "http://metadata.google.internal/computeMetadata/v1/secret-path",
            "http://metadata.goog/computeMetadata/v1/secret-path",
            "http://service.local/secret-path",
            "http://service.internal/secret-path",
        ] {
            let err = HttpPublisherConfig {
                target_url: blocked.to_string(),
                headers: None,
            }
            .normalized_with_local_network(false)
            .unwrap_err();
            assert!(
                matches!(err, InvalidPublisherConfig::InvalidTargetUrl { .. }),
                "expected InvalidTargetUrl for `{blocked}`, got {err:?}"
            );
            assert!(
                !err.to_string().contains("secret-path"),
                "leaked secret-path in `{err}`"
            );
        }
    }

    #[test]
    fn http_config_local_network_opt_in_allows_loopback_private_and_blocks_metadata() {
        for allowed_local in [
            "http://127.0.0.1:8080/hook",
            "http://[::1]:8080/hook",
            "http://localhost:8080/hook",
            "http://10.0.0.5/hook",
            "http://192.168.1.10/hook",
            "http://[fd00::1]/hook",
        ] {
            assert!(
                HttpPublisherConfig {
                    target_url: allowed_local.to_string(),
                    headers: None,
                }
                .normalized_with_local_network(true)
                .is_ok(),
                "expected `{allowed_local}` to be allowed with local network opt-in"
            );
        }

        for always_blocked in [
            "http://169.254.169.254/latest/meta-data/",
            "http://[::ffff:169.254.169.254]/",
            "http://[::ffff:0:169.254.169.254]/",
            "http://[2002:a9fe:a9fe::]/",
            "http://[fe80::1]/",
            "http://0.0.0.0/",
            "http://[::]/",
            "http://100.64.0.1/",
            "http://224.0.0.1/",
            "http://metadata.google.internal/",
            "http://metadata.goog/",
        ] {
            assert!(
                HttpPublisherConfig {
                    target_url: always_blocked.to_string(),
                    headers: None,
                }
                .normalized_with_local_network(true)
                .is_err(),
                "expected `{always_blocked}` to remain blocked even with local network opt-in"
            );
        }

        // Direct validation helper also rejects embedded userinfo credentials
        let with_userinfo = Url::parse("https://user:pass@subscriber.example.org/hook").unwrap();
        assert!(validate_http_target_url(&with_userinfo, true).is_err());
    }

    #[cfg(feature = "allow-localhost")]
    mod allow_localhost_tests {
        use super::*;

        #[test]
        fn allow_localhost_feature_permits_loopback_and_still_blocks_metadata() {
            assert!(HttpPublisherConfig {
                target_url: "http://127.0.0.1:8080/hook".to_string(),
                headers: None,
            }
            .normalized()
            .is_ok());

            assert!(HttpPublisherConfig {
                target_url: "http://localhost:8080/hook".to_string(),
                headers: None,
            }
            .normalized()
            .is_ok());

            assert!(HttpPublisherConfig {
                target_url: "http://169.254.169.254/latest/meta-data/".to_string(),
                headers: None,
            }
            .normalized()
            .is_err());
        }
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

        let ipv6_config = NatsPublisherConfig {
            nats_url: "NATS://[::1]:4222/".to_string(),
            subject: "unicore.events".to_string(),
        }
        .normalized()
        .unwrap();
        assert_eq!(ipv6_config.nats_url, "nats://[::1]:4222");

        for bad_subject in [
            "",
            "   ",
            "unicore..events",
            ".leading",
            "trailing.",
            "has space",
            "has\ttab",
            "has\0nul",
            "*",
            ">",
            "unicore.*",
            "unicore.>",
            "unicore.*.events",
            "unicore.events*",
            "unicore.events>",
        ] {
            assert!(NatsPublisherConfig {
                nats_url: "nats://127.0.0.1:4222".to_string(),
                subject: bad_subject.to_string(),
            }
            .normalized()
            .is_err());
        }

        for bad_url in [
            "",
            "http://127.0.0.1:4222",
            "nats://",
            "nats://@127.0.0.1:4222",
            "@127.0.0.1:4222",
            "nats://127.0.0.1:4222/secret-path",
            "nats://127.0.0.1:4222?token=secret-token",
            "nats://127.0.0.1:4222#secret-token",
            "nats://bro\tker:4222",
            "nats://127.0.0.1:4222\r\n/secret-path",
            "127.0.0.1:4222/secret-path?token=secret-token",
            "nats://user:password@127.0.0.1:4222",
            "nats://secret-token@127.0.0.1:4222",
            "http://user:password@127.0.0.1:4222",
            "mailto:secret-token@example.test",
            "urn:unicore:secret-token@example.test",
        ] {
            let err = NatsPublisherConfig {
                nats_url: bad_url.to_string(),
                subject: "unicore.events".to_string(),
            }
            .normalized()
            .unwrap_err();
            let rendered = err.to_string();
            assert!(!rendered.contains("password"), "leaked password in `{rendered}`");
            assert!(
                !rendered.contains("secret-token"),
                "leaked secret-token in `{rendered}`"
            );
            assert!(!rendered.contains("secret-path"), "leaked secret-path in `{rendered}`");
        }
    }

    #[test]
    fn http_config_debug_redacts_secret_headers_capability_paths_and_queries() {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_string(), "Bearer super-secret-value".to_string());
        for secret_url in [
            "https://user:url-secret@example.test/webhook",
            "https://hooks.example.test/cap/url-secret?sig=query-secret#frag-secret",
            "http://[::1]:8080/cap/url-secret?sig=query-secret",
            "mailto:url-secret@example.test",
            "urn:unicore:url-secret@example.test",
            "url-secret-raw-token",
        ] {
            let config = HttpPublisherConfig {
                target_url: secret_url.to_string(),
                headers: Some(headers.clone()),
            };
            let debug_output = format!("{config:?}");
            assert!(!debug_output.contains("super-secret-value"));
            assert!(
                !debug_output.contains("url-secret"),
                "leaked url-secret in `{debug_output}`"
            );
            assert!(
                !debug_output.contains("query-secret"),
                "leaked query-secret in `{debug_output}`"
            );
            assert!(
                !debug_output.contains("frag-secret"),
                "leaked frag-secret in `{debug_output}`"
            );
            assert!(debug_output.contains(REDACTED_HEADER_VALUE));
            assert!(debug_output.contains("<redacted>"));
        }

        for secret_nats_url in [
            "nats://user:nats-secret@127.0.0.1:4222",
            "nats://127.0.0.1:4222?token=nats-secret",
            "mailto:nats-secret@example.test",
            "urn:unicore:nats-secret@example.test",
        ] {
            let nats_config = NatsPublisherConfig {
                nats_url: secret_nats_url.to_string(),
                subject: "unicore.events".to_string(),
            };
            let nats_debug = format!("{nats_config:?}");
            assert!(!nats_debug.contains("nats-secret"));
            assert!(nats_debug.contains("<redacted>"));
        }

        let mut placeholder_headers = BTreeMap::new();
        placeholder_headers.insert("Authorization".to_string(), REDACTED_HEADER_VALUE.to_string());
        assert!(matches!(
            HttpPublisherConfig {
                target_url: "https://example.test/webhook".to_string(),
                headers: Some(placeholder_headers),
            }
            .normalized()
            .unwrap_err(),
            InvalidPublisherConfig::InvalidHeaderValue { .. }
        ));
    }

    #[test]
    fn filter_rejects_unknown_fields() {
        let json = serde_json::json!({
            "types": ["*"],
            "sources": ["/services/issuance"]
        });
        assert!(serde_json::from_value::<EventPublisherFilter>(json).is_err());
    }
}
