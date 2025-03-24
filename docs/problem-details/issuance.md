# Issuance

UniCore can be used for creating and issuing Verifiable Credentials based on defined configurations and client
requests. This process involves validating input data, applying the correct credential formats and types, and signing or
encoding the credentials for issuance. The errors documented in this section relate to possible failure scenarios
encountered during credential issuance, such as misconfigured credential configurations, invalid data in the credential
subject, or missing required elements.

### Invalid Credential Type

This error is raised when the credential provided in the request does not match the expected type based on its signed status. Specifically:

- **Signed Credentials:** The credential must be provided as a string representing a verifiable credential signed as a JWT.
- **Unsigned Credentials:** The credential must be provided as a JSON object.

If the credential's type does not meet these requirements, the API returns a `400 Bad Request` error.

#### Resolution

- For signed credentials, ensure that the credential value is a valid JWT containing a properly signed verifiable credential.
- For unsigned credentials, verify that the credential value is a well-formed JSON object with all the necessary fields for credential issuance.

Review your request payload to confirm that the credential type aligns with these expectations.

### No Credential Configuration Found

This error is raised when the system cannot find a credential configuration that matches the provided `credentialConfigurationId`. This indicates that either the specified configuration does not exist or it has not been properly set up in the server's settings.

#### Resolution

- **Verify the Identifier:**  
  Ensure that the `credentialConfigurationId` in your request is correct.

- **Review Server Configuration:**  
  Verify that the desired credential configuration is defined in the `credential_configurations` section of your `config.yaml` file. Ensure that the `credentialConfigurationId` provided in your request exactly matches the `credential_configuration_id` of the intended configuration.

## Unsupported Credential Format

<!-- TODO: We can eliminate this error type by creating a UniCore-specific `CredentialFormats` enum that only contains formats that are currently supported by UniCore. This would basically mean that an error would occur during startup (while constructing the `ApplicationConfig`) when an unsupported format is configured instead of during runtime. -->

<!-- TODO: Once issue https://github.com/impierce/ssi-agent/issues/136 is fixed this error type can probalably be removed because the credential configuration can be done during startup (from configuration) or during creating CredentialConfiguration aggregate instances through the API -->

This error is raised when UniCore encounters a credential configuration that specifies a format it cannot process. This
typically occurs when the credential configuration’s `format` does not match any of the supported formats.

### Resolution

Currently
UniCore only supports the `jwt_vc_json` format. This configuration needs to be set through the
`credential_configurations` property in the `config.yaml` file, e.g.:

```yaml
credential_configurations:
  - credential_configuration_id: my-credential-configuration-id
    format: jwt_vc_json
    credential_definition:
      type:
        - VerifiableCredential
```

## Unsupported Credential Type

This error is raised when UniCore encounters a credential type that is not recognized or supported by the current
implementation. In practice, it means that none of the credential types provided in the configuration match any of the types UniCore is configured to process. For example, supported types might include `VerifiableCredential`, `AchievementCredential`, or `OpenBadgeCredential`.

### Resolution

<!-- TODO: We need a dedicated documentation page explaining Credential Configurations and link to it here. Related issue: https://github.com/impierce/ssi-agent/issues/136 -->

<!-- TODO: Once issue https://github.com/impierce/ssi-agent/issues/136 is fixed this error type can probalably be removed because the credential configuration can be done during startup (from configuration) or during creating CredentialConfiguration aggregate instances through the API -->

UniCore supports three primary credential models:

- **Generic W3C Verifiable Credentials**
- **OpenBadgeV3 Credentials**
- **Custom W3C Verifiable Credentials**

For all models, the credential type must be declared in the `credential_definition` property of your credential
configuration (in the `config.yaml` file). Ensure that the configuration includes the base type `VerifiableCredential` along with
any additional types required by your credential model.

For example:

**For W3C Verifiable Credentials:**

```yaml
credential_configurations:
  - credential_configuration_id: my-w3c-vc-configuration-id
    format: jwt_vc_json
    credential_definition:
      type:
        - VerifiableCredential
```

**For OpenBadgeV3 Credentials:**

```yaml
credential_configurations:
  - credential_configuration_id: my-obv3-configuration-id
    format: jwt_vc_json
    credential_definition:
      type:
        - VerifiableCredential
        - OpenBadgeCredential
```

**For Custom W3C Verifiable Credentials:**

```yaml
credential_configurations:
  - credential_configuration_id: my-custom-w3c-vc-configuration-id
    format: jwt_vc_json
    credential_definition:
      type:
        - VerifiableCredential
        - MyCustomCredential
```

## Invalid Credential Subject

This error occurs when the `credentialSubject` provided during credential creation does not adhere to the expected format or schema. In other words, the data representing the credential's subject is either malformed, missing required properties, or contains values that fail to meet the validation criteria for the specified credential type.

### Resolution

<!-- TODO: link to API reference -->

While the `credentialSubject` is generally a JSON object that can contain arbitrary data, specific credential types may impose stricter requirements on its structure and content. For example, **OpenBadgeV3** credentials require a more defined schema (see [OpenBadgeV3 Achievement Credential Specification](https://www.imsglobal.org/spec/ob/v3p0#achievementcredential)).

To resolve this error, ensure that the `credentialSubject` conforms exactly to the expected structure as defined in the credential configuration corresponding to the submitted `credentialConfigurationId`.

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

## Invalid Credential Offer URI

This error is raised when UniCore fails to construct a valid URI to the Credential Offer ednpoint. It occurs during the process of joining the credential issuer’s base URL with required path segments and the `offerId`, and indicates that the resulting URI is malformed or invalid.

### Resolution

This error is not caused by incorrect client input but reflects a flaw in the internal processing of the Authorization
Request. If you encounter this error, please report it to the development team along with any relevant logs and context,
so that the issue can be investigated and resolved.
