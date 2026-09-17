use super::error::DocumentError;
use identity_did::CoreDID;
use url::{Host, Origin, Url};

/// Rejects origins that cannot identify a public domain: IP-addressed hosts and opaque origins
/// (e.g. `data:`), which have no host at all.
fn validate_public_origin(url: &Url) -> Result<(), DocumentError> {
    let Origin::Tuple(_, host, _) = url.origin() else {
        return Err(DocumentError::OpaqueOriginError);
    };
    if matches!(host, Host::Ipv4(_) | Host::Ipv6(_)) {
        return Err(DocumentError::HostError);
    }
    Ok(())
}

/// A deployment DID identifies the public origin, independently of the application's base path.
pub fn did_web(public_url: &Url) -> Result<CoreDID, DocumentError> {
    validate_public_origin(public_url)?;
    let Origin::Tuple(_, host, _) = public_url.origin() else {
        unreachable!("validated above");
    };
    let authority = match public_url.port() {
        Some(port) => format!("{host}%3A{port}"),
        None => host.to_string(),
    };
    format!("did:web:{authority}")
        .parse()
        .map_err(|err: identity_did::Error| DocumentError::InvalidDidError(err.to_string()))
}

/// Normalizes user-supplied domain or origin input (e.g. from a runtime configuration request) into
/// a validated [`Url`]. A bare host such as `example.org` is treated as `https://example.org`; a
/// full origin such as `http://example.org:8080` is parsed as given. Validated the same way
/// [`did_web`] validates `public_url`: IP hosts and opaque origins are rejected.
pub fn normalize_origin(input: &str) -> Result<Url, DocumentError> {
    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url: Url = candidate
        .parse()
        .map_err(|_| DocumentError::InvalidOriginError(input.to_string()))?;
    validate_public_origin(&url)?;
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_identifier_uses_domain_and_non_default_port() {
        for (url, expected) in [
            ("https://example.org", "did:web:example.org"),
            ("https://example.org:443/app/", "did:web:example.org"),
            ("http://example.org", "did:web:example.org"),
            ("http://example.org:80/app/", "did:web:example.org"),
            ("https://example.org:80", "did:web:example.org%3A80"),
            ("http://example.org:443", "did:web:example.org%3A443"),
            ("https://example.org:8443", "did:web:example.org%3A8443"),
            ("http://example.org:8080", "did:web:example.org%3A8080"),
        ] {
            assert_eq!(did_web(&url.parse().unwrap()).unwrap().to_string(), expected);
        }
    }

    #[test]
    fn rejects_ip_addresses_and_opaque_origins() {
        for url in ["https://127.0.0.1", "https://[::1]"] {
            assert!(matches!(did_web(&url.parse().unwrap()), Err(DocumentError::HostError)));
        }
        assert!(matches!(
            did_web(&"data:text/plain,hello".parse().unwrap()),
            Err(DocumentError::OpaqueOriginError)
        ));
    }

    #[test]
    fn normalizes_bare_hosts_and_full_origins() {
        for (input, expected) in [
            ("example.org", "https://example.org/"),
            ("example.org:8443", "https://example.org:8443/"),
            ("http://example.org:8080", "http://example.org:8080/"),
            ("https://example.org/unicore/", "https://example.org/unicore/"),
        ] {
            assert_eq!(normalize_origin(input).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn rejects_invalid_or_disallowed_origin_input() {
        assert!(matches!(
            normalize_origin("not a domain!!"),
            Err(DocumentError::InvalidOriginError(_))
        ));
        assert!(matches!(normalize_origin("127.0.0.1"), Err(DocumentError::HostError)));
        // Has no `://`, so it's treated as a bare host with a (non-numeric, invalid) port rather
        // than as the opaque `data:` URI it actually is — rejected either way.
        assert!(matches!(
            normalize_origin("data:text/plain,hello"),
            Err(DocumentError::InvalidOriginError(_))
        ));
        // A full origin whose scheme really is opaque is still rejected as such.
        assert!(matches!(
            normalize_origin("data://text/plain,hello"),
            Err(DocumentError::OpaqueOriginError)
        ));
    }
}
