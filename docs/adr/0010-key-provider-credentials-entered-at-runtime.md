# ADR 0010: Key Provider Credentials Are Entered at Runtime

**Status**: Proposed
**Date**: 2026-11-04
**Context**: Supporting key providers besides AWS KMS, starting with STACKIT KMS

---

## Context

The Key Management System generates and signs with keys in an external key provider. Adding a
provider is a runtime action: an operator adds it through the API, with no configuration edit and
no restart, as for linked domains (ADR 0006).

The one provider so far, AWS KMS, needs no secret from UniCore. It uses the credentials of the
environment UniCore runs in, an IAM role, which is why its type is `aws_kms_ambient`. That role
belongs to whoever hosts the deployment, and who grants it decides which keys the deployment may
use.

The next provider is STACKIT KMS, and there the credential belongs to the operator. The operator
connects their own STACKIT project, with a service account from their own STACKIT account, and can
reach their instance only through its API, not its environment. STACKIT authenticates a service
account with a key file: the client signs a short-lived JWT with the account's RSA private key and
exchanges it for an access token, which lasts about 15 minutes and comes without a refresh token.
The key file is a long-lived secret.

STACKIT binds roles on the organization, folder or project, not on a key ring or key, so a service
account can use every key in its project. A project per deployment is what keeps deployments' keys
apart.

So the question is where UniCore gets, and keeps, a credential that only the operator has.

## Options

1. **The credential comes from the environment**, as a variable or a mounted file. That works for
   whoever hosts the deployment, which is why it suits `aws_kms_ambient`, but an operator can't set
   it, and changing it means a restart.
2. **The operator enters the credential through the API, and UniCore stores it encrypted, outside
   the event stream.** It needs somewhere to keep the credential and a key to encrypt it with.
3. **The credential is part of the provider's event.** Rejected: events are immutable and kept for
   good, and they are published to the event publishers, so the secret would leave the instance and
   could never be removed.

## Decision

Option 2, for every provider type whose credential belongs to the operator, starting with STACKIT
KMS. Provider types whose credential belongs to the host, such as `aws_kms_ambient`, keep reading it
from the environment.

- The operator enters the credential when adding the provider, together with the settings that
  aren't secret, such as STACKIT's project, region and key ring.
- The credential is kept behind a credential store port, which the KMS reads it from when a
  provider needs it. Its first implementation keeps it in Stronghold, which every deployment
  already has, encrypted with the deployment's Stronghold password, next to the issuer keys. When
  Stronghold is replaced, only the implementation behind the port changes.
- The provider's events record the settings and that a credential was given, never the credential
  itself.
- The operator replaces a credential, for example after rotating a STACKIT key, through the API as
  well, without a restart. Removing a provider removes its credential.

## Consequences

- No secret appears in UniCore's events, and none reaches the event publishers.
- An operator can connect their own key provider without access to the hosting environment.
- Whoever holds a deployment's Stronghold password can read the credentials of its providers, as
  they already can its issuer keys. Losing the Stronghold snapshot loses the credentials too, and
  the operator has to enter them again.
- The credential store is a new port that a Stronghold replacement must also provide.
- Isolating deployments within a provider remains the operator's concern: for STACKIT, by giving
  each deployment its own project and service account.
