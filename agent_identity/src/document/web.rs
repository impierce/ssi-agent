use super::error::DocumentError;
use identity_did::CoreDID;
use url::{Host, Origin, Url};

/// A deployment DID identifies the public origin, independently of the application's base path.
pub fn did_web(public_url: &Url) -> Result<CoreDID, DocumentError> {
    let Origin::Tuple(_, host, _) = public_url.origin() else {
        return Err(DocumentError::OpaqueOriginError);
    };
    if matches!(host, Host::Ipv4(_) | Host::Ipv6(_)) {
        return Err(DocumentError::HostError);
    }
    let authority = match public_url.port() {
        Some(port) => format!("{host}%3A{port}"),
        None => host.to_string(),
    };
    format!("did:web:{authority}")
        .parse()
        .map_err(|err: identity_did::Error| DocumentError::InvalidDidError(err.to_string()))
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
}
