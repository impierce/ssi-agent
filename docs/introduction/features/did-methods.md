# DID Methods

## DID Resolution and Public Key Verification

A decentralized identifier (DID) serves as a pointer to a corresponding DID Document. This document contains the public key(s) that verifiers use to confirm the authenticity of signed data. In practice, when you present a DID, it allows any verifier to look up your associated DID Document and retrieve your public keys. These keys are then used to check that any data you’ve signed with your private key is valid and hasn’t been tampered with. This mechanism is a core part of ensuring trust and security in self-sovereign identity systems.

## DID Methods supported in UniCore

- **did:key**
- **did:jwk**
- **did:web**
- **did:iota**

### Self-contained DID Methods

DID Methods such as **did:key** and **did:jwk** are self-contained, meaning that the public key information needed for
verifying signatures is embedded directly within the identifier itself. This design enables immediate resolution without
needing to query an external service. Although these DID Methods are very simple and easy to use, they have some limitations:

- **Immutable Key Information:** Once created, the embedded key cannot be updated. This poses a challenge if you need to rotate keys or update cryptographic parameters due to evolving security standards.
- **Limited Flexibility:** Self-contained DID Methods don't allow any updates to the corresponding DID Document. This
  means you cannot easily extend or modify the associated data after the DID is created, limiting adaptability in
  dynamic environments. Features like **Domain Linkage** and **Linked Verifiable Presentations** are therefore not possible.

In contrast, DID Methods like **did:web** and **did:iota** are more flexible and extensible. They allow you to store the
DID Document in a separate location, such as a web server or a distributed ledger. This separation enables you to update
the DID Document independently of the DID itself, providing more flexibility and control over the associated data.

### Web-hosted vs. Ledger-based DID Methods

The **did:web** method leverages established web infrastructure by hosting DID Documents on domains, which results in human-readable identifiers and seamless integration with existing DNS and HTTPS systems. This approach offers familiarity and ease of use for many applications. In contrast, the **did:iota** method anchors its DID Documents on the IOTA Tangle, a distributed ledger that provides additional benefits aligned with self-sovereign identity (SSI) principles.

- **Enhanced Decentralization and Autonomy:** **did:iota** reduces reliance on centralized web hosting and DNS services, giving users and organizations greater control over their digital identities.
- **Robust Security Through Distributed Trust:** The trustless verification process of distributed ledgers offers strong security benefits. This model allows for independent verification of identity data, complementing the straightforward approach of did:web with an extra layer of security.

:::note

The DID Documents, regardless of the method used, contain only the public key(s) and optionally associated features such
as **Domain Linkage** and **Linked Verifiable Presentations**. Their primary role is to enable the verification of signed data.
Importantly, they do not store any actual personal information on the ledger or web server.

:::

## Configuration

Customize the supported DID Methods and signing algorithms for your UniCore instance using the `config.yaml` file. This
file allows you to enable or disable individual DID Methods according to your specific requirements. Note that one DID
Method must always be marked as `preferred`.

Example:

```yaml
did_methods:
  did_jwk:
    enabled: true
  did_key:
    enabled: true
  did_iota:
    preferred: true
    enabled: true
  did_web:
    enabled: true

domain_linkage_enabled: true

signing_algorithms_supported:
  es256:
    preferred: true
    enabled: true
  eddsa:
    enabled: true
```

:::info

Domain Linkage can be enabled by setting `domain_linkage_enabled` to `true`. This feature applies only to
non-self-contained DID Methods. For more information about this feature, see [Domain
Linkage](/docs/unicore/deployment/domain-linkage.md).

:::

:::info

Signing algorithms can be enabled and disabled by setting the `signing_algorithms_supported` property. The `preferred`
property indicates the default signing algorithm used by UniCore.

:::
