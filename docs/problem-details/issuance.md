# Issuance

UniCore can be used for creating and issuing Verifiable Credentials based on defined configurations and client
requests. This process involves validating input data, applying the correct credential formats and types, and signing or
encoding the credentials for issuance. The errors documented in this section relate to possible failure scenarios
encountered during credential issuance, such as misconfigured credential configurations, invalid data in the credential
subject, or missing required elements.

## Invalid Credential Type

This error is raised when the credential provided in the request does not match the expected type based on its signed status. Specifically:

- **Signed Credentials:** The credential must be provided as a string representing a verifiable credential signed as a JWT.
- **Unsigned Credentials:** The credential must be provided as a JSON object.

If the credential's type does not meet these requirements, the API returns a `400 Bad Request` error.

### Resolution

- For signed credentials, ensure that the credential value is a valid JWT containing a properly signed verifiable credential.
- For unsigned credentials, verify that the credential value is a well-formed JSON object with all the necessary fields for credential issuance.

Review your request payload to confirm that the credential type aligns with these expectations.

## No Credential Configuration Found

This error is raised when the system cannot find a credential configuration. The credential configurations are derived from the templates. This error indicates that either the template does not exist, is not in a usable state, or its derived credential configuration is unavailable. If you believe this is an unexpected error, please consider opening an [issue on GitHub](https://github.com/impierce/ssi-agent/issues).

### Resolution

- **Review Template State:**  
  Verify that the target template exists and is `Published` before issuing credentials or creating offers.

## Unsupported Credential Format

<!-- TODO: We can eliminate this error type by creating a UniCore-specific `CredentialFormats` enum that only contains formats that are currently supported by UniCore. This would basically mean that an error would occur during startup (while constructing the `ApplicationConfig`) when an unsupported format is configured instead of during runtime. -->

<!-- TODO: Once issue https://github.com/impierce/ssi-agent/issues/136 is fixed this error type can probalably be removed because the credential configuration can be done during startup (from configuration) or during creating CredentialConfiguration aggregate instances through the API -->

This error is raised when UniCore encounters a credential configuration that specifies a format it cannot process. This
typically occurs when the credential configuration’s `format` does not match any of the supported formats.

### Resolution

Since credential configurations and formats are not manually managed through the API, please consider opening an issue on GitHub if you believe this is an unexpected error.

## Invalid Credential Payload

This error occurs when the credential data inside the payload provided during credential creation does not adhere to the expected format or schema. In other words, the data representing the credential is either malformed, missing required properties, or contains values that fail to meet the validation criteria for the specified credential type.

### Resolution

<!-- TODO: link to API reference -->

While the `credentialSubject` is generally a JSON object that can contain arbitrary data, specific credential types may impose stricter requirements on its structure and content. For example, **OpenBadgeV3** credentials require a more defined schema (see [OpenBadgeV3 Achievement Credential Specification](https://www.imsglobal.org/spec/ob/v3p0#achievementcredential)).

To resolve this error, ensure that `credentialSubject` conforms exactly to the schema defined by the target Template
(or by the referenced provisioned credential configuration, if you are using provisioned mode).

## Invalid Identifier Error

This error indicates that the identifier provided in the credential data is invalid. It is raised when the `id` field is missing or its value cannot be parsed as a valid URI.

### Resolution

<!-- TODO: link to API reference -->

Ensure that the `id` field is a properly formatted string representing a valid URI. Note that the requirement for an
`id` field varies by credential type: for **W3C Verifiable Credentials** the `id` is optional, while for **OpenBadgeV3**
credentials it is required.

**Examples of valid `id` values:**

```json
{
  "id": "https://example.com/credentials/123",
  "credentialSubject": {
    ...
  }
}
```

```json
{
  "id": "uurn:uuid:6e8bc430-9c3a-11d9-9669-0800200c9a66",
  "credentialSubject": {
    ...
  }
}
```

## Invalid Expiration Date

<!-- TODO: link to API reference -->

This error occurs when the `expiresAt` value provided for the credential is invalid. It is typically triggered when the expiration date exceeds the maximum allowed value (`9999-12-31T23:59:59Z`) or when it does not conform to the expected date-time format.

### Resolution

Ensure that the `expiresAt` value is a valid date-time string and falls within the acceptable range (up to `9999-12-31T23:59:59Z`). If the credential should not expire, you may also set `expiresAt` to `"never"`, although providing an explicit expiration date is generally recommended.

:::info

The `expiresAt` field accepts either a valid date-time string or the special value `"never"` to indicate that the
credential does not expire.

:::

## Missing Credential Offer

This error occurs when UniCore cannot locate a Credential Offer for the provided `offerId`. It typically indicates that an operation is being attempted on a Credential Offer that has not been created or is otherwise unavailable.

### Resolution

Verify that a Credential Offer with the specified `offerId` exists before proceeding. If the Credential Offer is missing, create it using the `POST /v0/offers` endpoint.

<!-- TODO: Add link to Reference API -->

## Send Credential Offer Error

This error is raised when UniCore fails to send the Credential Offer to the specified target URL. It is triggered
when the HTTP request does not complete successfully.

### Resolution

Check the target URL and ensure that it is reachable and correctly configured to receive the Credential Offer.

## Update Provisioned Credential Configuration Error

This error occurs when an attempt is made to update a provisioned credential configuration during runtime. Provisioned credential configurations are immutable after initial setup, and any update operation will result in this error. The API returns a `400 Bad Request` error.

### Resolution

Provisioned credential configurations cannot be updated after they are set up. If you need to change a configuration, you must update your server's configuration files and restart the service.

## Remove Provisioned Credential Configuration Error

This error is returned when there is an attempt to remove a provisioned credential configuration during runtime. Provisioned credential configurations cannot be removed after they have been set up. The API returns a `400 Bad Request` error.

### Resolution

Provisioned credential configurations cannot be removed at runtime. To remove a configuration, update your server's configuration files and restart the service.

## Unsupported Credential Format Identifier Error

This error occurs when the `format` specified in a credential configuration is not supported by UniCore. This typically happens when the format does not match any of the recognized credential formats.

### Resolution

Ensure that the `format` field in the credential configuration is set to a supported value. Supported values include: `jwt_vc_json` and `dc+sd-jwt`.

## Missing Template ID

This error occurs when a credential issuance request omits the `templateId` field or provides it as an empty string. Issuance requires exactly one template so the API can resolve the template state, schema, and derived credential configuration.

### Resolution

Set `templateId` to the identifier of an existing template.

## Missing Template IDs

This error occurs when an offer creation request omits the `templateIds` field, provides an empty array, or includes empty template identifiers. Offer creation requires at least one valid template identifier.

### Resolution

Provide a non-empty `templateIds` array and ensure every element is a non-empty template identifier.

## Template Not Found

This error occurs when the request references a template identifier that does not exist or refers to a template that has already been deleted. UniCore cannot issue credentials or create offers without first resolving the backing template.

### Resolution

- Verify that the requested template still exists.
- Ensure the template identifier is spelled correctly.
- If the template was deleted, create a new template or use a different published template.

## Template Not Published

This error occurs when a request references a template that exists but is not in the `Published` state. UniCore only allows published templates to be used for credential issuance and offer generation.

### Resolution

- Publish the template before using it in issuance or offers.
- If the template is intentionally in `Draft` or `Archived`, switch to a published template instead.

## Invalid Signed Credential Format

This error occurs when a signed credential payload is structurally invalid for the expected signed-credential parsing flow. Typical causes include malformed JWTs, invalid base64url segments, invalid JSON in JWT segments, a missing `vc` claim for `jwt_vc_json`, or an SD-JWT with an unsupported `typ` header.

### Resolution

- Ensure the signed credential is a syntactically valid JWT or SD-JWT.
- For `jwt_vc_json`, include a `vc` claim in the JWT payload.
- For SD-JWT formats, set the issuer JWT header `typ` to `vc+sd-jwt` or `dc+sd-jwt`.

## Signed Credential Format Mismatch

This error occurs when the signed credential supplied in the request uses a different format than the format derived from the selected template's credential configuration. For example, supplying a `jwt_vc_json` credential for a template that resolves to `vc+sd-jwt` will trigger this error.

### Resolution

- Use a signed credential whose format matches the selected template's credential configuration.
- If the wrong template was selected, retry with the template that matches the signed credential format.

## Unsupported Credential Configuration Format

This error occurs when UniCore derives a credential configuration from a template, but that derived configuration uses a credential format the signed-credential validation pipeline does not support.

### Resolution

This indicates a server-side inconsistency. Review the template and derived credential configuration, and update them to use a supported format.

## Expiration Exceeds Template Limit

This error occurs when the `expiresAt` value requested for a credential exceeds the expiration defined by the template. For example, requesting `never` or a later timestamp than the template allows will be rejected.

### Resolution

- Omit `expiresAt` to use the template default.
- If you provide `expiresAt`, ensure it is not later than the template's allowed deadline.

## Invalid Template Schema

This error occurs when a template contains a `schema` value that is not itself a valid JSON Schema. In that case UniCore cannot validate incoming credential data against it.

### Resolution

- Fix the template schema so it is valid JSON Schema.
- Re-publish or update the template before retrying issuance.

## Credential Schema Validation Failed

This error occurs when the credential data in the request does not satisfy the template's JSON Schema. UniCore validates unsigned credential payloads against the template schema before creating the credential and returns the list of schema violations in the error detail.

### Resolution

- Inspect the reported validation violations.
- Update the credential payload so it satisfies the template schema.
- When using a schema for credential-subject fields, ensure the request data under `credential.credentialSubject` matches that schema.

## Template Not Eligible For Public Offer

This error occurs when a template is used for a public offer but its schema contains non-constant leaf values. Public offers are only allowed for templates whose schema fully constrains leaf values with `const`, so the resulting credential content is predetermined.

### Resolution

- Restrict the template schema to constant leaf values only.
- If the credential content must remain dynamic, use a regular offer flow instead of a public offer.
