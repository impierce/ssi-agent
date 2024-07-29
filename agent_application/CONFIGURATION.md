# Configuration

A configuration file is used to configure UniCore. It is expected to be present in `agent_application/config.yaml`. An example can be found in [example-config.yaml](example-config.yaml). Values can also be set through the environment, preferably used to inject sensitive values or environment-specific values.

> [!NOTE]
> Environment variables override values specified in the configuration file.

## Common

| Name                                  | Description                                                               | Default value | Accepted values                                                    |
| ------------------------------------- | ------------------------------------------------------------------------- | ------------- | ------------------------------------------------------------------ |
| `LOG_FORMAT`                          | The format of the log output.                                             | `json`        | `json`, `text`                                                     |
| `EVENT_STORE`                         | The type of event store to use.                                           | -             | `in-memory`, `postgres`                                            |
| `EVENT_STORE_DB_CONNECTION_STRING`    | The connection string for the event store database.                       | -             | `postgresql://<user>:<pass>@<host>` (only required for `postgres`) |
| `URL`                                 | The URL of the service itself.                                            | -             | `https://my-domain.example.org`                                    |
| `CORS_ENABLED`                        | Enable CORS (permissive, allow all). Only required for web-based wallets. | `false`       | boolean                                                            |
| `DID_METHOD_WEB_ENABLED`              | Create and host a `did:web` document.                                     | `false`       | boolean                                                            |
| `DOMAIN_LINKAGE_ENABLED`              | Enable domain linkage (only works with `did:web`).                        | `false`       | boolean                                                            |
| `EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS` | The timeout for external server responses.                                | `1000`        | integer                                                            |
| `PREFERRED_DID_METHOD`                | The default DID method to use.                                            | `jwk`         | `jwk`, `key`, `web`                                                |

## Stronghold

| Name                  | Description                            | Default value | Accepted values               |
| --------------------- | -------------------------------------- | ------------- | ----------------------------- |
| `STRONGHOLD_PATH`     | The path to the stronghold file.       | -             | `/var/lib/unicore/stronghold` |
| `STRONGHOLD_PASSWORD` | The password to unlock the stronghold. | -             | -                             |
| `ISSUER_DID`          | The DID of the issuer.                 | -             | -                             |
| `ISSUER_FRAGMENT`     | The fragment to be used.               | -             | -                             |
| `KEY_ID`              | The key ID to be used.                 | -             | -                             |

## Look and Feel

| Name                  | Description                       | Default value | Accepted values |
| --------------------- | --------------------------------- | ------------- | --------------- |
| `CREDENTIAL_NAME`     | The name of the credential.       | -             | string          |
| `CREDENTIAL_LOGO_URL` | The URL of the credential's logo. | -             | URL             |
