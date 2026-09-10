# Linked Domains

Domain linkage proves that the controller of a DID also controls a web origin. UniCore publishes signed
Domain Linkage Credentials at `/.well-known/did-configuration.json` and adds a `LinkedDomains`
service to its enabled, update-supporting DID documents (`did:web` and funded or sponsored `did:iota`).

Which domains are linked is **independent of the deployment's own identity**: the DID being proven and
the origins being claimed are separate concepts, exactly as they are for `did:iota`, whose identifier
has no textual relationship to any domain. Linking a domain never changes the deployment DID.

## Deployment identity

UniCore derives its deployment `did:web` from the origin of `public_url`, which defaults to
`application_url`. The public URL must be externally reachable. The scheme and application path
are not part of the identifier. Default ports are omitted; non-default ports are percent encoded:

| Public URL | Deployment DID |
| --- | --- |
| `https://example.org/unicore/` | `did:web:example.org` |
| `http://example.org:80/` | `did:web:example.org` |
| `https://example.org:8443/` | `did:web:example.org%3A8443` |

HTTP normalization does not remove the protocols' HTTPS requirements for production verification.
IP addresses cannot be used as `did:web` hosts.

The document is persisted at creation and reused on restart, including its keys and services.
Linking, renewing, or unlinking a domain never changes the DID or rotates its signing keys.
After creation, the persisted DID is the source of truth for the deployment identity.

If the configured origin would produce a different DID than the one persisted, startup fails and
reports the persisted DID and the newly configured origin. This prevents an ordinary deployment
configuration change from silently invalidating the identity referenced by issued credentials.

A deliberate migration requires `overwrite_previous_did_web` to exactly match the persisted DID
while `public_url` identifies the new origin:

```yaml
public_url: https://new.example.org
overwrite_previous_did_web: did:web:old.example.org
```

The equivalent environment variable is
`UNICORE__OVERWRITE_PREVIOUS_DID_WEB=did:web:old.example.org`. On the next startup UniCore records a
`DocumentDidWebOverwritten` event, retains the existing keys and services, renews the active Domain
Linkage credentials, and logs the old and new identifiers. Remove the override
after that successful startup; a stale value cannot authorize another migration because it no
longer matches the persisted DID.

Overwriting the DID does not migrate previously issued credentials. They still reference the old
DID, so the operator must keep the old DID document resolvable at its old origin for as long as
those credentials must remain verifiable.

## Runtime commands

Linked domains are managed entirely through the API, at runtime. Nothing here requires an environment
variable change or a redeploy, which is what lets a tenant of a hosted deployment set up their own
domain themselves. The former `domain_linkage_enabled` setting is ignored with a deprecation warning.

| Command | Effect |
| --- | --- |
| `POST /v0/add-linked-domains` | Link one or more origins; body: `{"origins":["example.org"]}`. |
| `POST /v0/remove-linked-domains` | Unlink one or more origins; body: `{"origins":["example.org"]}`. |
| `GET /v0/verify-linked-domains` | Check every linked domain the way an external verifier would. |
| `POST /v0/create-linked-verifiable-presentation` | Add a linked presentation service; body: `{"presentationIds":["presentation-1"]}`. |
| `POST /v0/remove-linked-verifiable-presentation` | Remove the linked presentation service, retaining the presentations themselves. No request body. |

Each entry in `origins` is either a bare host (`example.org`, read as `https://example.org`) or a full
origin (`http://example.org:8080`). Only the origin is kept: any path is discarded, so
`https://example.org/app/` and `https://example.org` link the same domain. IP addresses and opaque
origins are rejected. One invalid entry rejects the whole request with `400`, rather than linking the
rest.

Both commands return `204` and are **idempotent, with no cap and no conflicts**: linking a domain that
is already linked is a no-op, and so is unlinking one that is not linked. An empty `origins` array is a
`400`. Linking requires an eligible signing DID, and returns `400` when none is enabled; unlinking
deliberately does not, so a domain can still be withdrawn after `did:web` has been disabled.

### What gets published

Each Domain Linkage Credential claims exactly one origin, so linking *N* domains with *M* enabled
signing keys publishes *N × M* credentials in a single `/.well-known/did-configuration.json`. A
verifier checking one origin accepts that document as soon as one credential matches the origin it
fetched from, and ignores the rest — so every linked domain can serve the same document.

The `LinkedDomains` service entry follows the specification's two permitted shapes: a single linked
domain is written as a bare origin string, and multiple domains as an object with an `origins` array.

```jsonc
// one linked domain
"serviceEndpoint": "https://example.org/"
// several linked domains
"serviceEndpoint": { "origins": ["https://example.org/", "https://foo.example.org/"] }
```

Unlinking one of several domains keeps the remaining domains' credentials exactly as they were issued,
rather than re-signing them. Unlinking the last one withdraws the service and the published
configuration entirely; `/.well-known/did-configuration.json` then returns `404` again.

### Pointing a domain at this deployment

A linked domain must resolve to this deployment so that it serves the same
`/.well-known/did-configuration.json`. Point it with a `CNAME` record at the deployment's own host —
the host of `public_url`:

```dns
example.org.  CNAME  your-deployment.example.net.
```

### Verifying

`GET /v0/verify-linked-domains` returns `200` with a result per linked domain:

```json
{
  "valid": false,
  "origins": [
    {
      "origin": "https://example.org/",
      "valid": true,
      "linkage_valid": true,
      "dns": { "points_here": true, "chain": ["your-deployment.example.net."] }
    },
    {
      "origin": "https://foo.example.org/",
      "valid": false,
      "linkage_valid": false,
      "dns": { "points_here": false, "chain": [] },
      "message": "Failed to fetch the published domain linkage configuration: ...; No CNAME record found; expected one pointing to 'your-deployment.example.net'"
    }
  ]
}
```

For each origin UniCore fetches `/.well-known/did-configuration.json` from that origin exactly as an
external verifier would — proving DNS, HTTPS and the `/.well-known/` hosting are genuinely reachable —
and checks it against the DIDs it linked to *that* origin. Alongside it, the origin's `CNAME` chain is
resolved with caching disabled, so a record edited moments ago is seen immediately.

`valid` reflects the linkage check, not the DNS check. A correctly served domain therefore verifies
even when `dns.points_here` is `false`: an apex domain **cannot** have a `CNAME` record and must use a
provider's `ALIAS`/`ANAME` or CNAME-flattening instead. The DNS result is reported because it is
usually the reason a linkage check failed, and it names what to fix. Top-level `valid` is `true` only
when every linked domain verified; when nothing is linked at all, it is `false` with a `message` and an
empty `origins` list.

Credentials are valid for 365 days. UniCore checks at startup and hourly, renewing when 30 days or less
remain. Renewal re-signs the linked set without changing it, reusing the same DID and keys. Runtime
maintenance failures are logged and retried on the next hourly check. Unlinked domains are never
renewed or restored on restart.

Requests use the configured actor extraction and authorization checker. Service reads remain under
`GET /v0/services`.

## Hosting with an application base path

Both `/.well-known/did.json` and `/.well-known/did-configuration.json` are served at the domain
root, even when the application uses a base path. For example, with a `/unicore/` base path,
commands are under `/unicore/v0/`, while the two identity resources remain under `/.well-known/`.

Configure the reverse proxy to forward these root paths to UniCore as well as the application path.
The DID configuration endpoint returns `404` before any domain is linked and after the last one is
unlinked. Linking a domain updates the published configuration without restarting.

See the [DID Configuration specification](https://identity.foundation/well-known-did-configuration/resources/did-configuration/)
and the [did:web method specification](https://w3c-ccg.github.io/did-method-web/).
