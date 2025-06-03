# Configuration

UniCore can be configured using a configuration file or via environment variables or a combination of both.

## Configuration file

By default, UniCore looks for a `config.yaml` file in the application root folder on startup. A path to a different config file can be specified using the `UNICORE__CONFIG_FILE` environment variable. An example config file can be found in [example.config.yaml](https://github.com/impierce/ssi-agent/blob/beta/agent_application/example.config.yaml). Using a config file generally gives a better overview over the configuration, while environment variables are commonly used to inject sensitive values or set deployment-specific values in a CI.

:::info

Environment variables **override** values specified in the configuration file. This allows you to define a base
configuration in a file and override specific values using environment variables.

:::

:::note

All environment variables need to be prefixed with `UNICORE__` to prevent conflicts with other unrelated variables.

:::

## Default and provisioned values

UniCore uses a sensible default configuration to reduce initial setup friction. If you override any of the default values by supplying your own values via a config file or environment variables, those values are treated as **provisioned** config values. Provisioned values cannot be changed during runtime to ensure consistency across deployments and restarts.

## Runtime configuration

:::warning

Changing the configuration at runtime via the API is currently **not supported**, but will be possible in the near future.

:::

## Inspecting the current configuration

UniCore serves its configuration at the `/v0/configuration` endpoint. Sensitive values are redacted. In case you're interested in all provisioned values, a `?provisioned=true` query parameter can be added to the URL.

## Configuration options

Find the full list of UniCore's configuration options below.

:::info

This overview is still a work in progress. You can refer to the [example.config.yaml](https://github.com/impierce/ssi-agent/blob/beta/agent_application/example.config.yaml) file for a more comprehensive list of options.

:::

### URL

UniCore's base URL. This value is communicated to clients and identity wallets.

| Environment variable | `config.yaml` |
| -------------------- | ------------- |
| `UNICORE__URL`       | `url`         |

#### Example

```yaml
url: https://my-domain.example.test
```

### Port

The port UniCore's HTTP server listens on.

| Environment variable | `config.yaml` |
| -------------------- | ------------- |
| `UNICORE__PORT`      | `port`        |

#### Values

- `3033` _(default)_

#### Example

```yaml
port: 1337
```

### Log format

The format of the log output.

| Environment variable  | `config.yaml` |
| --------------------- | ------------- |
| `UNICORE__LOG_FORMAT` | `log_format`  |

#### Values

- `json` _(default)_
- `text`

#### Example

```yaml
log_format: text
```

### Event store

The event store is used to persist events and serves as UniCore's persistence layer.

| Environment variable                      | `config.yaml`                   |
| ----------------------------------------- | ------------------------------- |
| `UNICORE__EVENT_STORE__TYPE`              | `event_store.type`              |
| `UNICORE__EVENT_STORE__CONNECTION_STRING` | `event_store.connection_string` |

#### Values

##### `type`

- `postgres` _(default)_
- `in_memory`

##### `connection_string`

Only required when `type` is `postgres`.

#### Example

```yaml
event_store:
  type: postgres
  connection_string: postgresql://user:password@database:5432/demo
```

<!--
## More configuration options

| Name                                                    | Description                                                       | Default value | Accepted values |
| ------------------------------------------------------- | ----------------------------------------------------------------- | ------------- | --------------- |
| `UNICORE__BASE_PATH`                                    | A base path can be set if needed.                                 | -             | string          |
| `UNICORE__CORS_ENABLED`                                 | Enable CORS (permissive). Only required for browser-based access. | `false`       | boolean         |
| `UNICORE__DID_METHODS__DID_WEB__ENABLED`                | Create and host a `did:web` DID document.                         | `false`       | boolean         |
| `UNICORE__SIGNING_ALGORITHMS_SUPPORTED__EDDSA__ENABLED` | Toggles the algorithm allowed for cryptographic operations.       | `true`        | boolean         |
| `UNICORE__DOMAIN_LINKAGE_ENABLED`                       | Enable domain linkage (only works with `did:web`).                | -             | boolean         |
| `UNICORE__EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS`          | The timeout for external server responses (in milliseconds).      | `1000`        | integer         |
-->

<!-- TODO: How to document all other DID methods? -->
<!-- TODO: VP_FORMATS -->
<!-- TODO: EVENT_PUBLISHERS: even configured through env vars? -->

<!--
## Secret Management

| Name                                           | Description                                       | Default value | Accepted values               |
| ---------------------------------------------- | ------------------------------------------------- | ------------- | ----------------------------- |
| `UNICORE__SECRET_MANAGER__STRONGHOLD_PATH`     | The path to the stronghold file.                  | -             | `/var/lib/unicore/stronghold` |
| `UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD` | The password to unlock the stronghold.            | -             | -                             |
| `UNICORE__SECRET_MANAGER__ISSUER_EDDSA_KEY_ID` | The key ID of the EDDSA (Ed25519) key to be used. | -             | -                             |
| `UNICORE__SECRET_MANAGER__ISSUER_ES256_KEY_ID` | The key ID of the ES256 key to be used.           | -             | -                             |
-->

## Look and Feel

:::info

Setting display values is currently not supported through environment variables. Please refer to `config.yaml`.

:::

<!-- TODO: DISPLAY_0_NAME: even configured through env vars? -->
