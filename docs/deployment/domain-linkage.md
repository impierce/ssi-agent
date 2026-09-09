# Domain Linkage

Domain linkage proves that the controller of a DID also controls a web origin. UniCore publishes signed
Domain Linkage Credentials at `/.well-known/did-configuration.json` and adds a `LinkedDomains`
service to its enabled, update-supporting DID documents (`did:web` and funded or sponsored `did:iota`).

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
Enabling, renewing, or removing domain linkage never changes the DID or rotates its signing keys.
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
`DocumentDidWebOverwritten` event, retains the existing keys and services, renews active Domain
Linkage credentials for the new origin, and logs the old and new identifiers. Remove the override
after that successful startup; a stale value cannot authorize another migration because it no
longer matches the persisted DID.

Overwriting the DID does not migrate previously issued credentials. They still reference the old
DID, so the operator must keep the old DID document resolvable at its old origin for as long as
those credentials must remain verifiable.

## Runtime commands

Domain linkage is enabled through the API. It is not created or deleted by startup configuration.
The former `domain_linkage_enabled` setting is ignored with a deprecation warning.

| Command | Effect |
| --- | --- |
| `POST /v0/create-domain-linkage` | Create linkage to the origin of `public_url`. No request body. |
| `POST /v0/remove-domain-linkage` | Remove linkage and its entries from DID documents. No request body. |
| `GET /v0/verify-domain-linkage` | Resolve the linkage UniCore currently publishes and validate it externally. |
| `POST /v0/create-linked-verifiable-presentation` | Add a linked presentation service; body: `{"presentationIds":["presentation-1"]}`. |
| `POST /v0/remove-linked-verifiable-presentation` | Remove the linked presentation service, retaining the presentations themselves. No request body. |

Commands return `204` on success, `409` when creating an existing service, and `404` when removing
a missing service. Creating linkage without an eligible signing DID returns `400`.
`verify-domain-linkage` always returns `200` with `{"valid": boolean, "message": string | null}`:
it fetches `/.well-known/did-configuration.json` from `public_url` exactly as an external verifier
would (proving DNS, HTTPS, and hosting are reachable) and checks it against the DID(s) UniCore
expects to have linked. `message` is `null` on success and otherwise describes what failed, e.g. an
unreachable origin or a DID missing from the published configuration.
Requests use the configured actor extraction and authorization checker. The old
`POST /v0/services/linked-vp` endpoint has been removed. Service reads remain under `GET /v0/services`.

Linkage credentials are valid for 365 days. UniCore checks at startup and hourly, renewing when
30 days or less remain. Renewal uses the same DID and keys. Runtime maintenance failures are logged
and retried on the next hourly check. Removed services are not renewed or restored on restart.
Domain linkage can subsequently be created again through the API.

## Hosting with an application base path

Both `/.well-known/did.json` and `/.well-known/did-configuration.json` are served at the domain
root, even when the application uses a base path. For example, with a `/unicore/` base path,
commands are under `/unicore/v0/`, while the two identity resources remain under `/.well-known/`.

Configure the reverse proxy to forward these root paths to UniCore as well as the application path.
The DID configuration endpoint returns `404` before linkage is created and after it is removed.
Creating domain linkage updates the published configuration without restarting.

See the [DID Configuration specification](https://identity.foundation/well-known-did-configuration/resources/did-configuration/)
and the [did:web method specification](https://w3c-ccg.github.io/did-method-web/).
