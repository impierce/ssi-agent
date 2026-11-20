# ADR 0011: Key Providers Sign In With the Host's Workload Identity

**Status**: Proposed
**Date**: 2026-11-16
**Context**: Supporting key providers besides AWS KMS, starting with STACKIT KMS

---

## Context

The Key Management System generates and signs with keys in an external key provider. Adding a
provider is a runtime action: an operator adds it through the API, with no configuration edit and
no restart, as for linked domains (ADR 0006).

The one provider so far, AWS KMS, needs no secret from UniCore. It uses the credentials of the
environment UniCore runs in, an IAM role, which is why its type is `aws_kms_ambient`.

The next provider is STACKIT KMS, and there a customer may connect their own STACKIT project. UniCore
then acts as a service account in that project. STACKIT lets a service account sign in in two ways:

- **Key flow:** the client signs a JWT with the RSA private key of a service account key file, and
  exchanges it for an access token. The key file is a long-lived secret.
- **Workload identity federation:** the service account trusts an external OpenID Connect issuer.
  The client presents a short-lived token from that issuer and exchanges it for an access token.
  STACKIT recommends this flow.

Access tokens last about 15 minutes, and neither flow returns a refresh token.

STACKIT binds roles on the organization, folder or project, not on a key ring or key, so a service
account can use every key in its project. A project per deployment is what keeps deployments' keys
apart.

UniCore is moving away from Stronghold, so there is no longer a place in UniCore to keep a secret
such as a key file encrypted.

## Options

1. **The customer enters the key file through the API, and UniCore stores it encrypted, outside the
   event stream.** It needs a secret store and a key to encrypt with, which UniCore won't have once
   Stronghold is gone, and it leaves a long-lived secret that the customer has to rotate.
2. **The key file comes from the environment.** Only whoever hosts the deployment can set it, it
   changes only with a restart, and it is still a long-lived secret.
3. **Workload identity federation, with UniCore as its own OpenID Connect issuer.** Every deployment
   is its own issuer, but it needs public discovery and key set endpoints, and a signing key that
   itself has to be kept somewhere.
4. **Workload identity federation, with the identity of the workload UniCore runs as.** On
   Kubernetes, the cluster issues each pod a short-lived token for its service account, from the
   cluster's own OpenID Connect issuer. UniCore stores no long-lived secret.

## Decision

Option 4, for STACKIT KMS.

- The operator adds a `stackit_kms` provider through the API with its settings: the project,
  region and key ring, and the email of the service account UniCore signs in as. None of them are
  secret, so they are recorded in the provider's event.
- No secret is entered through the API, recorded in an event, or stored by UniCore.
- UniCore reads the token its host provides, from a file whose path is deployment configuration,
  and exchanges it at STACKIT for an access token, again whenever the access token is about to
  expire.
- The customer sets up their side in their own STACKIT project: a service account with access to
  the key ring, and a federated identity provider on it that trusts the host's issuer. They get the
  issuer and the subject from whoever hosts the deployment. The audience is always
  `sts.accounts.stackit.cloud`: the host issues UniCore's token for it.
- The federated identity provider must check the subject as well as the audience. The issuer is
  shared by every workload on the cluster, and the subject
  (`system:serviceaccount:<namespace>:<service account>`) is what identifies this deployment. A
  federated identity provider without a subject check lets any workload on the cluster act as the
  service account. Setting it up correctly is the customer's responsibility, and the documentation
  for adding a STACKIT provider says so.
- A key file is used only for local development and tests, never through the API.
- `aws_kms_ambient` keeps using the IAM role of its environment.

## Consequences

- No long-lived secret appears in UniCore's events, storage or API, so there is no key for UniCore to
  lose, leak or rotate, and nothing that depends on Stronghold. The host's token and the access
  token still grant access while they are valid, for minutes: the host keeps the token file readable
  only to UniCore, and UniCore logs neither token.
- The customer revokes UniCore's access by removing the federated identity provider, without
  involving UniCore.
- STACKIT KMS works only where the host gives UniCore a workload identity that STACKIT can trust,
  which for now means the deployments Impierce runs on its Kubernetes cluster. Deployments
  elsewhere, such as on the customer's own premises, can't use STACKIT KMS yet.
- A customer who leaves out the subject check opens their service account to other workloads on the
  same cluster. UniCore can't detect this, as it can't read the customer's federation settings.
- Isolating deployments within a provider remains the customer's concern: for STACKIT, by giving
  each deployment its own project and service account.
