## Credential Types and Verifiable Formats

Credentials can be issued in 4 different **Digital Credential Data Formats**:
- [Verifiable Credential Data Model 1.1](https://www.w3.org/TR/vc-data-model-1.1) (VC DM 1.1)
- [Verifiable Credential Data Model 2.0](https://www.w3.org/TR/vc-data-model/) (VC DM 2.0)
- [Open Badges 3.0](https://www.imsglobal.org/spec/ob/v3p0) (OB 3.0)
- [European Digital Credential 3.3](https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model) (ELM 3.3)
> For more resources on these specifications please refer to the `README.md` file at https://github.com/impierce/ssi-agent/tree/beta/agent_library/src/json_schemas/README.md.

Furthermore, there are 3 supported formats in which these credentials can be made verifiable, all of which are **envelopping methods**:
- [W3C Verifiable Credentials 1.1](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-w3c-verifiable-credentials) (`jwt_vc_json`)
- [SD-JWT VC](https://datatracker.ietf.org/doc/draft-ietf-oauth-sd-jwt-vc/) (`dc+sd-jwt`)
- [VC DM 2.0 SD-JWT](https://www.w3.org/TR/vc-jose-cose/#with-sd-jwt) (`vc+sd-jwt`)

Due to certain restrictions in the above mentioned formats, not all data formats can be combined with each envelopping method. The following combinations are possible:
| Envelopping Method | Data Format |
|-------------------|-------------|
| W3C Verifiable Credentials 1.1 | VC DM 1.1 |
| W3C Verifiable Credentials 1.1 | OB 3.0 |
| W3C Verifiable Credentials 1.1 | ELM 3.3 |
| SD-JWT VC | SD-JWT VC |
| VC DM 2.0 SD-JWT | VC DM 2.0 |
| VC DM 2.0 SD-JWT | OB 3.0 |
| VC DM 2.0 SD-JWT | ELM 3.3 |

## Manual credential reissuance

`POST /v0/reissue-credential` prepares an SD-JWT replacement, a new OpenID4VCI offer,
and an audit relation linking the original and replacement. The holder redeems the
returned offer through the normal OpenID4VCI flow; preparing the replacement does not
immediately issue it.

The original must already be Issued. The requested `credentialConfigurationId` is the
backing template ID and must resolve to the same credential format and type as the
original, including its SD-JWT type parameters. Supported formats are `dc+sd-jwt` and
`vc+sd-jwt`. Display metadata may change between issuances. The template must exist,
remain Published, and accept the supplied payload under its current JSON Schema.
Validation uses `credentialSubject` when present, otherwise the payload itself.
`expiresAt` is optional and defaults to the template's credential expiration. When
provided, it must be an RFC 3339 timestamp within the template's current expiration
deadline, or `"never"` when the template has no deadline.

For an issued original under a published `SD-JWT VC` template with no expiration limit:

```http
POST /v0/reissue-credential
Content-Type: application/json

{
  "originalCredentialId": "11111111-1111-4111-8111-111111111111",
  "credentialConfigurationId": "SD-JWT VC",
  "credential": { "first_name": "Ferris", "last_name": "Reissued", "dob": "2010-01-01" },
  "expiresAt": "never",
  "reason": "data_changed",
  "triggerType": "manual",
  "triggeredBy": "operator"
}
```

A successful response is 201 and contains `id`, `originalCredentialId`,
`newCredentialId`, `offerId`, `credentialConfigurationId`, and `credentialOffer` (the
encoded offer to deliver to the holder). Query audit relations with
`GET /v0/get-credential-reissuance/{id}` or `GET /v0/list-all-credential-reissuances`.
These queries return the stored aggregate fields in snake_case, with `id` as the
relation identifier. Trigger fields are optional audit metadata; `triggeredBy` does
not establish authority. Each underlying command and query checks the request caller.
Application integrations can inject a `ReissuancePolicy` into `ReissuanceService::new`
for additional restrictions; the HTTP endpoint uses the default policy.

Reissuance does not revoke or suspend the original. Use the existing credential-status
update endpoint when that is required. `statusAction` is unsupported and is rejected
as an unknown request field. Historical events and views containing `status_action`
remain readable; that obsolete metadata is ignored and omitted from new output.

Validation failures occur before writes. The relation is persisted before the new
credential and offer, so a failed relation command cannot expose an unlinked replacement.
The workflow spans multiple aggregates and is not atomic: a later failure can leave a
relation whose replacement or offer is incomplete. Inspect the relation, credential,
and offer before retrying. The HTTP endpoint generates fresh identifiers on every call
and does not provide idempotent retries. Persistence conflicts return 503; template,
schema, and credential errors retain the same problem types as normal issuance.

For PostgreSQL, new databases receive `reissuance` and `all_reissuances` view tables
from `agent_application/docker/db/init.sql`. Existing databases must have both added
before enabling this endpoint. Apply the two corresponding `CREATE TABLE` definitions
from that initialization file to the existing database; do not rerun the full file.
The command-line `psql` client can apply the selected definitions after review. Each
table requires `view_id text PRIMARY KEY`, `version bigint NOT NULL CHECK (version >= 0)`,
and `payload json NOT NULL`. Restart the application after applying the schema update.
