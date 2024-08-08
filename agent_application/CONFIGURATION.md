# Configuration

A configuration file is used to configure UniCore. It is expected to be present in `agent_application/config.yaml`. An example can be found in [example-config.yaml](example-config.yaml). Values can also be set through the environment, preferably used to inject sensitive values or environment-specific values.

> [!NOTE]
> Environment variables **override** values specified in the configuration file.

> [!IMPORTANT]
> All environment variables need to be prefixed with `UNICORE__` to prevent conflicts with existing variables.

## General

| Name                                                    | Description                                                       | Default value | Accepted values                          |
| ------------------------------------------------------- | ----------------------------------------------------------------- | ------------- | ---------------------------------------- |
| `UNICORE__LOG_FORMAT`                                   | The format of the log output.                                     | `json`        | `json`, `text`                           |
| `UNICORE__EVENT_STORE__TYPE`                            | The type of event store to use.                                   | -             | `in_memory`, `postgres`                  |
| `UNICORE__EVENT_STORE__CONNECTION_STRING`               | The connection string for the event store database.               | -             | `postgresql://<user>:<pass>@<host>/<db>` |
| `UNICORE__URL`                                          | The base URL UniCore runs on.                                     | -             | `https://my-domain.example.org`          |
| `UNICORE__BASE_PATH`                                    | A base path can be set if needed.                                 | -             | string                                   |
| `UNICORE__CORS_ENABLED`                                 | Enable CORS (permissive). Only required for browser-based access. | `false`       | boolean                                  |
| `UNICORE__DID_METHODS__DID_WEB__ENABLED`                | Create and host a `did:web` DID document.                         | `false`       | boolean                                  |
| `UNICORE__SIGNING_ALGORITHMS_SUPPORTED__EDDSA__ENABLED` | Toggles the algorithm allowed for cryptographic operations.       | `true`        | boolean                                  |
| `UNICORE__DOMAIN_LINKAGE_ENABLED`                       | Enable domain linkage (only works with `did:web`).                | -             | boolean                                  |
| `UNICORE__EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS`          | The timeout for external server responses (in milliseconds).      | `1000`        | integer                                  |

<!-- TODO: How to document all other DID methods? -->
<!-- TODO: VP_FORMATS -->
<!-- TODO: EVENT_PUBLISHERS: even configured through env vars? -->

## Secret Management

| Name                                           | Description                            | Default value | Accepted values               |
| ---------------------------------------------- | -------------------------------------- | ------------- | ----------------------------- |
| `UNICORE__SECRET_MANAGER__STRONGHOLD_PATH`     | The path to the stronghold file.       | -             | `/var/lib/unicore/stronghold` |
| `UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD` | The password to unlock the stronghold. | -             | -                             |
| `UNICORE__SECRET_MANAGER__ISSUER_KEY_ID`       | The key ID to be used.                 | -             | -                             |
| `UNICORE__SECRET_MANAGER__ISSUER_DID`          | The DID of the issuer.                 | -             | -                             |
| `UNICORE__SECRET_MANAGER__ISSUER_FRAGMENT`     | The fragment to be used.               | -             | -                             |

## Look and Feel

> [!NOTE]
> Setting display values is currently not supported through environment variables. Please refer to `config.yaml`.

<!-- TODO: DISPLAY_0_NAME: even configured through env vars? -->
