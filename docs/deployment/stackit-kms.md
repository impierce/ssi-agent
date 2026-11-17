# STACKIT KMS

With the [`kms` feature](../configuration/CONFIGURATION.md#key-management-system-kms), UniCore can keep its keys in
a key ring of a STACKIT project. Keys are generated in STACKIT KMS and sign there; the private keys never leave it.
The project can be the customer's own.

UniCore acts in the project as a STACKIT service account. It doesn't sign in with a key file or any other secret:
it presents the workload identity token its host gives it, and the service account trusts that identity through
workload identity federation. See [ADR 0011](../adr/0011-key-providers-sign-in-with-workload-identity.md) for why.

Setting this up takes three parties:

1. Whoever hosts the deployment gives UniCore a workload identity token, and tells the customer its issuer and
   subject.
2. The customer sets up their STACKIT project to trust that identity.
3. An operator adds the provider to UniCore through the API.

## 1. The host: a workload identity token

STACKIT trusts tokens from an OpenID Connect issuer whose discovery document and key set are public. On Kubernetes
this is the cluster's service account issuer, such as the OIDC issuer of an EKS cluster.

Project a service account token for STACKIT into UniCore's pod, with the audience `sts.accounts.stackit.cloud`:

```yaml
spec:
  serviceAccountName: unicore
  containers:
    - name: unicore
      volumeMounts:
        - name: stackit-token
          mountPath: /var/run/secrets/stackit.cloud/serviceaccount
          readOnly: true
  volumes:
    - name: stackit-token
      projected:
        sources:
          - serviceAccountToken:
              path: token
              audience: sts.accounts.stackit.cloud
              expirationSeconds: 600
```

UniCore reads the token from `/var/run/secrets/stackit.cloud/serviceaccount/token`, where STACKIT's own SDKs look
for it too. Kubernetes renews the file, and UniCore reads it again each time it signs in. To read it from
elsewhere, set:

| Environment variable                     | `config.yaml`                   | Default                                               |
| ---------------------------------------- | ------------------------------- | ----------------------------------------------------- |
| `UNICORE__STACKIT_FEDERATED_TOKEN_FILE` | `stackit_federated_token_file` | `/var/run/secrets/stackit.cloud/serviceaccount/token` |

Give the customer:

- the **issuer**: the cluster's OIDC issuer URL;
- the **subject**: `system:serviceaccount:<namespace>:<service account>` of this deployment's pod.

## 2. The customer: the STACKIT project

In the STACKIT project that will keep the keys:

1. Create a key ring in a region that offers KMS (`eu01`).
2. Create a service account for UniCore, and give it a role on the project that lets it create, read, sign with
   and delete KMS keys. STACKIT grants roles per project, not per key ring, so the service account can use every
   key in the project. Use a project for each deployment to keep deployments' keys apart.
3. Add a federated identity provider to the service account, with the issuer the host gave you and these
   assertions:

   | Item  | Value                                               |
   | ----- | --------------------------------------------------- |
   | `aud` | `sts.accounts.stackit.cloud`                        |
   | `sub` | the subject the host gave you, such as `system:serviceaccount:unicore-acme:unicore` |

> [!WARNING]
> The `sub` assertion is required, even though STACKIT only demands `aud`. Every workload on the host's cluster
> gets tokens from the same issuer, so without it, any of them could act as your service account. UniCore can't
> see your federation settings, so it can't warn you about this. Getting them right is your responsibility.

To revoke UniCore's access, remove the federated identity provider or the service account's role. UniCore needs
no change for that.

## 3. The operator: adding the provider

Add the provider with the project, region, key ring and service account. None of these are secret, and they are
stored with the provider:

```http
POST /v0/add-key-provider
Content-Type: application/json

{
  "providerType": "stackit_kms",
  "name": "ACME STACKIT",
  "settings": {
    "projectId": "4a9b0c2e-0d8f-4b4c-9e65-0e5d3c2a1b10",
    "region": "eu01",
    "keyRingId": "7c1e9f3a-5b2d-4e8f-a6c4-2d9b8e7f6a50",
    "serviceAccountEmail": "unicore-1a2b3c@sa.stackit.cloud"
  }
}
```

Then check that UniCore can sign in and reach the key ring:

```http
POST /v0/test-key-provider-connection
Content-Type: application/json

{ "providerType": "stackit_kms" }
```

When it can't, the response's `message` says why, such as a missing token file, a sign-in STACKIT refused, or a key
ring that doesn't exist in the project. To use another project or key ring, remove the provider and add it again.

## Keys

- Keys are P-256 and sign with ES256. Other algorithms are refused.
- New keys get STACKIT's `software` protection.
- A key created through UniCore is recorded as `<key id>/<version>`, which names one version of the key for good.
  Signing with a key named by its key id alone uses its newest version that can sign.
- A label becomes the key's display name in STACKIT. It can be up to 64 letters, digits, underscores and hyphens,
  and not a UUID. STACKIT doesn't keep display names unique, so UniCore only gives a label that no key in the key
  ring has, apart from deleted ones.
- UniCore waits, up to 30 seconds, until a new key has a version that can sign before it answers.
- Deleting a key schedules its deletion: STACKIT keeps it for 30 days, during which it can be restored in STACKIT.
  UniCore treats it as deleted right away.
- Listing a provider's keys shows the key ring's ES256 signing keys, and whether each can be used.
