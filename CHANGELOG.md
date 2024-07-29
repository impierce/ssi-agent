### 29-07-2024
- Renamed env variable prefix from `AGENT` to `UNICORE`.
- Refactored the environment variables separators from `_` to `__` to support nested configuration values. As an example, `AGENT_LOG_FORMAT` now becomes `UNICORE__LOG_FORMAT`.
- Merged all per-module configuration files into a single `config.yaml` file.

### 24-06-2024
- Reverted the API version to V0, which means that all endpoints previously prefixed with `/v1` are now prefixed with `/v0`.
- Changed `AGENT_APPLICATION_URL` to `AGENT_CONFIG_URL`.

### 20-06-2024
Deprecated the following environment variables:
* `AGENT_ISSUANCE_CREDENTIAL_NAME`
* `AGENT_ISSUANCE_CREDENTIAL_LOGO_URL`

Both can now be dynamically configured through the `/v1/configurations/credential_configurations` endpoint. Example:
```json
// HTTP POST: /v1/configurations/credential_configurations
{
  "display": [{
    "name": "Identity Credential", // <-- Credential Name
        "locale": "en",
        "logo": {
          "url": "https://impierce.com/images/logo-blue.png", // <-- Credential Logo URL
            "alt_text": "UniCore Logo"
        }
    }],
    "credentialConfigurationId": ...,
    "format": ...,
    "credential_definition": ...
}
```

### 18-06-2024
Deprecated the following environment variables, which can now be configured in the `agent_application/config.yml` file:
* `AGENT_CONFIG_DEFAULT_DID_METHOD`: The first item in the `subject_syntax_types_supported` sequence will be used as the
  default DID Method
* `AGENT_CONFIG_DISPLAY_NAME`: The display name can now be configured through `display` -> `name` in the `agent_application/config.yml` file
* `AGENT_CONFIG_DISPLAY_LOGO_URI`": The display logo URI can now be configured through `display` -> `logo` -> `uri` in the `agent_application/config.yml` file

### 23-04-2024
Renamed `subjectId` to `offerId`. This has effect on both the `/v1/credentials` and `/v2/offers` endpoints.

The `/v1/credentials` endpoint now accepts an object or a string as the `credential` value (previously it accepted only
objects). It also accepts an optional `isSigned` parameter, which indicates that the credential is already signed and
does not need to be signed in UniCore.

### 11-04-2024
`/v1/offers` incorrectly returned with Content-Type `application/json`. The Content-Type has now been changed to `application/x-www-form-urlencoded`.

### 24-01-2024

Environment variable `AGENT_APPLICATION_HOST` has changed to `AGENT_APPLICATION_URL` and requires the complete URL. e.g.:
`https://my.domain.com/unicore`. In case you don't have rewrite root enabled on your reverse proxy, you will have to set `AGENT_CONFIG_BASE_PATH` as well. e.g.: `unicore`.
