# Domain Linkage

Domain Linkage is a mechanism within self-sovereign identity (SSI) systems that securely binds a digital
identity — typically represented by a [Decentralized Identifier (DID)](https://www.w3.org/TR/did-core/) — to a specific web
domain. This binding is achieved by publishing cryptographic proofs or verifiable credentials on the domain, often in a
standardized location (e.g., a `.well-known` URL). The result is a trusted association that allows any verifier to
confidently confirm that the entity controlling the domain is the same one represented by the digital identity. This
process is crucial for enhancing trust and ensuring secure interactions in SSI applications.

:::note

Not all DID methods are suitable for Domain Linkage.
Only methods that allow updating DID Documents can support this mechanism.

| DID method | Domain Linkage supported |
| ---------- | :----------------------: |
| `did:iota` |            ✅            |
| `did:jwk`  |            ❌            |
| `did:key`  |            ❌            |
| `did:web`  |            ✅            |

:::

## Enabling Domain Linkage in UniCore

To enable Domain Linkage in UniCore, follow these steps:

- **Environment Variable:**  
  Set the `UNICORE__DOMAIN_LINKAGE_ENABLED` environment variable to `true`.

- **Configuration File:**  
  Alternatively, set `domain_linkage_enabled` to `true` in the `config.yaml` file.

When Domain Linkage is enabled, UniCore will generate and publish the necessary cryptographic proofs on the domain and
create the appropriate DID Documents for the enabled DID methods.

:::warning

Because the DID Configuration resource must reside at the domain root (see [DID Configuration
Spec](https://identity.foundation/specs/did-configuration/#resource_location)), Domain Linkage in UniCore will not work
if the `UNICORE__PUBLIC_URL` environment variable contains a path segment. For example, Domain Linkage will **not become active** for:

```bash
UNICORE__PUBLIC_URL=http://my-domain.com/unicore/
```

Instead, it must be configured as:

```bash
UNICORE__PUBLIC_URL=http://my-domain.com/
```

:::
