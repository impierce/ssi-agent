//! Fresh CNAME resolution, used to check that a linked domain's DNS actually points at this
//! deployment.
//!
//! A tenant links a domain by pointing it at the deployment with a `CNAME` record. Confirming that
//! record is what turns an opaque "could not fetch" into an actionable "your DNS is not set up yet",
//! so lookups here deliberately bypass every cache: a tenant who has just edited their DNS expects
//! the next verification attempt to see it.

use async_trait::async_trait;
use hickory_resolver::{
    proto::rr::{RData, RecordType},
    Resolver, TokioResolver,
};
use std::sync::Arc;
use tracing::{debug, warn};
use url::{Host, Url};

/// Resolves the `CNAME` chain of a host.
#[async_trait]
pub trait CnameResolver: Send + Sync {
    /// The chain of canonical names `host` points at, nearest first, or an error describing why the
    /// lookup could not be performed. A host with no `CNAME` record resolves to an empty chain,
    /// which is not an error: apex domains cannot have one.
    async fn cname_chain(&self, host: &str) -> Result<Vec<String>, String>;
}

/// A [`CnameResolver`] backed by the system's configured nameservers, with caching disabled.
pub struct SystemCnameResolver {
    resolver: TokioResolver,
}

impl SystemCnameResolver {
    pub fn new() -> Result<Self, String> {
        let mut builder = Resolver::builder_tokio().map_err(|error| error.to_string())?;
        // Verification must observe DNS edits the tenant made moments ago, so nothing is cached.
        builder.options_mut().cache_size = 0;
        Ok(Self {
            resolver: builder.build().map_err(|error| error.to_string())?,
        })
    }
}

#[async_trait]
impl CnameResolver for SystemCnameResolver {
    async fn cname_chain(&self, host: &str) -> Result<Vec<String>, String> {
        match self.resolver.lookup(host, RecordType::CNAME).await {
            Ok(lookup) => Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::CNAME(cname) => Some(cname.0.to_string()),
                    _ => None,
                })
                .collect()),
            // A host without a `CNAME` is reported as an empty answer rather than a failure; only a
            // genuine resolution problem (NXDOMAIN, timeout, misconfigured resolver) is an error.
            Err(error) if error.is_no_records_found() => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// A resolver that reports why it could not be constructed.
///
/// Lets a deployment whose resolver configuration is unusable still start and serve everything else;
/// only the DNS diagnostic in linked-domain verification degrades, and it says why.
struct UnavailableCnameResolver(String);

#[async_trait]
impl CnameResolver for UnavailableCnameResolver {
    async fn cname_chain(&self, _host: &str) -> Result<Vec<String>, String> {
        Err(self.0.clone())
    }
}

/// The system resolver, or a resolver that reports the configuration error if one cannot be built.
pub fn system_resolver() -> Arc<dyn CnameResolver> {
    match SystemCnameResolver::new() {
        Ok(resolver) => Arc::new(resolver),
        Err(error) => {
            warn!("DNS resolution is unavailable; linked domain CNAME checks will report this: {error}");
            Arc::new(UnavailableCnameResolver(format!(
                "the DNS resolver could not be initialized: {error}"
            )))
        }
    }
}

/// Whether `chain` leads to `target`, comparing host names as DNS does: case-insensitively and
/// ignoring the root's trailing dot.
pub fn chain_points_to(chain: &[String], target: &str) -> bool {
    let normalize = |host: &str| host.trim_end_matches('.').to_ascii_lowercase();
    let target = normalize(target);
    chain.iter().any(|name| normalize(name) == target)
}

/// The host a URL addresses, as a string suitable for DNS lookup. IP hosts have no `CNAME` and are
/// rejected before reaching here (see [`crate::document::web::normalize_origin`]).
pub fn lookup_host(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(domain) => Some(domain.to_owned()),
        Host::Ipv4(_) | Host::Ipv6(_) => None,
    }
}

/// The outcome of checking one origin's DNS against the deployment's own host.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct CnameCheck {
    /// Whether a `CNAME` record for the origin leads to the deployment.
    pub points_here: bool,
    /// The canonical names the origin points at, for diagnosing a failed check.
    pub chain: Vec<String>,
    /// Why the chain could not be resolved at all, as opposed to resolving but not matching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CnameCheck {
    /// Resolves `origin`'s `CNAME` chain and reports whether it leads to `deployment`.
    pub async fn resolve(resolver: &Arc<dyn CnameResolver>, origin: &Url, deployment: &Url) -> Self {
        let (Some(host), Some(target)) = (lookup_host(origin), lookup_host(deployment)) else {
            return Self {
                points_here: false,
                chain: Vec::new(),
                error: Some("Only domain names can be checked; IP hosts have no CNAME record".into()),
            };
        };

        match resolver.cname_chain(&host).await {
            Ok(chain) => {
                debug!("CNAME chain for '{host}': {chain:?}");
                Self {
                    points_here: chain_points_to(&chain, &target),
                    chain,
                    error: None,
                }
            }
            Err(error) => Self {
                points_here: false,
                chain: Vec::new(),
                error: Some(error),
            },
        }
    }

    /// The reason this check failed, phrased for whoever configured the domain.
    pub fn failure(&self, deployment: &Url) -> Option<String> {
        if self.points_here {
            return None;
        }
        let target = lookup_host(deployment).unwrap_or_else(|| deployment.to_string());
        Some(match (&self.error, self.chain.is_empty()) {
            (Some(error), _) => format!("Could not resolve DNS: {error}"),
            (None, true) => format!("No CNAME record found; expected one pointing to '{target}'"),
            (None, false) => format!(
                "CNAME points to {} instead of '{target}'",
                self.chain
                    .iter()
                    .map(|name| format!("'{}'", name.trim_end_matches('.')))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_matching_ignores_case_and_the_root_dot() {
        let chain = vec!["Deployment.Impierce.COM.".to_string()];

        assert!(chain_points_to(&chain, "deployment.impierce.com"));
        assert!(chain_points_to(&chain, "deployment.impierce.com."));
        assert!(chain_points_to(&chain, "DEPLOYMENT.impierce.com"));
    }

    #[test]
    fn chain_matching_rejects_other_hosts_and_empty_chains() {
        assert!(!chain_points_to(&[], "deployment.impierce.com"));
        assert!(!chain_points_to(
            &["elsewhere.example".to_string()],
            "deployment.impierce.com"
        ));
        // A suffix must not be mistaken for the host itself.
        assert!(!chain_points_to(
            &["impierce.com".to_string()],
            "deployment.impierce.com"
        ));
        assert!(!chain_points_to(
            &["evil-deployment.impierce.com".to_string()],
            "deployment.impierce.com"
        ));
    }

    #[test]
    fn chain_matching_accepts_a_longer_chain_that_reaches_the_target() {
        let chain = vec![
            "intermediate.example.".to_string(),
            "deployment.impierce.com.".to_string(),
        ];

        assert!(chain_points_to(&chain, "deployment.impierce.com"));
    }

    #[test]
    fn lookup_host_rejects_ip_addresses() {
        assert_eq!(
            lookup_host(&"https://example.org/unicore/".parse().unwrap()),
            Some("example.org".to_string())
        );
        assert_eq!(lookup_host(&"https://127.0.0.1".parse().unwrap()), None);
        assert_eq!(lookup_host(&"https://[::1]".parse().unwrap()), None);
    }

    struct StubResolver(Result<Vec<String>, String>);

    #[async_trait]
    impl CnameResolver for StubResolver {
        async fn cname_chain(&self, _host: &str) -> Result<Vec<String>, String> {
            self.0.clone()
        }
    }

    fn resolver(outcome: Result<Vec<String>, String>) -> Arc<dyn CnameResolver> {
        Arc::new(StubResolver(outcome))
    }

    #[tokio::test]
    async fn a_matching_cname_passes_without_a_failure_message() {
        let deployment: Url = "https://deployment.impierce.com/unicore/".parse().unwrap();
        let check = CnameCheck::resolve(
            &resolver(Ok(vec!["deployment.impierce.com.".into()])),
            &"https://tenant.example".parse().unwrap(),
            &deployment,
        )
        .await;

        assert!(check.points_here);
        assert_eq!(check.failure(&deployment), None);
    }

    #[tokio::test]
    async fn each_failure_mode_explains_itself() {
        let deployment: Url = "https://deployment.impierce.com/".parse().unwrap();
        let origin: Url = "https://tenant.example".parse().unwrap();

        let missing = CnameCheck::resolve(&resolver(Ok(vec![])), &origin, &deployment).await;
        assert!(!missing.points_here);
        assert!(missing.failure(&deployment).unwrap().contains("No CNAME record found"));

        let wrong = CnameCheck::resolve(&resolver(Ok(vec!["elsewhere.example.".into()])), &origin, &deployment).await;
        assert!(!wrong.points_here);
        let message = wrong.failure(&deployment).unwrap();
        assert!(message.contains("'elsewhere.example'"), "{message}");
        assert!(message.contains("'deployment.impierce.com'"), "{message}");

        let unresolvable = CnameCheck::resolve(&resolver(Err("NXDOMAIN".into())), &origin, &deployment).await;
        assert!(!unresolvable.points_here);
        assert!(unresolvable.failure(&deployment).unwrap().contains("NXDOMAIN"));
    }

    #[tokio::test]
    async fn an_ip_host_is_reported_as_uncheckable_rather_than_resolved() {
        let deployment: Url = "https://127.0.0.1:3033/".parse().unwrap();
        let check = CnameCheck::resolve(
            &resolver(Ok(vec!["deployment.impierce.com.".into()])),
            &"https://tenant.example".parse().unwrap(),
            &deployment,
        )
        .await;

        assert!(!check.points_here);
        assert!(check.error.is_some());
    }
}
