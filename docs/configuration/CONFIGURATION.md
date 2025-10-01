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

### Application URL

UniCore's application URL. This value represents the self-aware URL of the application. It is used for internal
communication and should not be exposed to clients or identity wallets.

:::note

The `UNICORE__APPLICATION_URL` may include a path segment, which will be treated as the base path for the application. All endpoints will be attached relative to this base path. For example, if you set `UNICORE__APPLICATION_URL` to `http://localhost:3033/my/base/path`, then all API endpoints will be served under `/my/base/path`.

:::

| Environment variable       | `config.yaml`     |
| -------------------------- | ----------------- |
| `UNICORE__APPLICATION_URL` | `application_url` |

#### Example

```yaml
application_url: http://localhost:3033/my/base/path
```

### Public URL

UniCore's public URL. This value is communicated to clients and identity wallets and should be publicly accessible. When
not set, it defaults to the value of `UNICORE__APPLICATION_URL`.

<!-- TODO: In production, should we require that `UNICORE__PUBLIC_URL` is always explicitly set, rather than defaulting to the value of `UNICORE__APPLICATION_URL`? This would help prevent accidental exposure of internal URLs to clients. -->

:::note

The `UNICORE__PUBLIC_URL` may also include a path segment, which will be treated as the base path for all public endpoints. For example, if you set `UNICORE__PUBLIC_URL` to `https://my-domain.example.test/my/base/path`, then all public API endpoints will be served under `/my/base/path`.

:::

| Environment variable  | `config.yaml` |
| --------------------- | ------------- |
| `UNICORE__PUBLIC_URL` | `public_url`  |

#### Example

```yaml
public_url: https://my-domain.example.test
```

<!-- TODO: We should add a better explanation to describe the difference between the `UNICORE__APPLICATION_URL` and the `UNICORE__PUBLIC_URL`, possibly with some diagrams. Is this the right place for that? -->

### Token Endpoint

The OAuth2/OpenID Connect token endpoint. This endpoint is used by clients to exchange authorization codes for access tokens.

:::note

This variable is **optional**. By default, the `UNICORE__PUBLIC_URL` is used as the base, and the `/auth/token` segment is appended to form the token endpoint URL. You can completely override this default by explicitly setting the `UNICORE__TOKEN_ENDPOINT` variable or the `token_endpoint` config value.

In most setups, the default value is recommended and usually the best choice.

:::

| Environment variable      | `config.yaml`    |
| ------------------------- | ---------------- |
| `UNICORE__TOKEN_ENDPOINT` | `token_endpoint` |

#### Example (default)

If `UNICORE__PUBLIC_URL` is set to `https://my-domain.example.test`, the default token endpoint will be:

```
https://my-domain.example.test/auth/token
```

#### Example (explicit override)

```yaml
token_endpoint: https://my-domain.example.test/custom/token/path
```

### Credential Endpoint

The endpoint where credentials can be issued to clients. This is typically used in credential issuance flows.

:::note

This variable is **optional**. By default, the `UNICORE__PUBLIC_URL` is used as the base, and the `/openid4vci/credential` segment is appended to form the credential endpoint URL. You can completely override this default by explicitly setting the `UNICORE__CREDENTIAL_ENDPOINT` variable or the `credential_endpoint` config value.

In most setups, the default value is recommended and usually the best choice.

:::

| Environment variable           | `config.yaml`         |
| ------------------------------ | --------------------- |
| `UNICORE__CREDENTIAL_ENDPOINT` | `credential_endpoint` |

#### Example (default)

If `UNICORE__PUBLIC_URL` is set to `https://my-domain.example.test`, the default credential endpoint will be:

```
https://my-domain.example.test/openid4vci/credential
```

#### Example (explicit override)

```yaml
credential_endpoint: https://my-domain.example.test/custom/credential/path
```

### Credential Offer URI

The URI used to represent a credential offer. This is communicated to clients to initiate credential issuance.

:::note

This variable is **optional**. By default, the `UNICORE__PUBLIC_URL` is used as the base, and the `/credential-offer` segment is appended to form the credential offer URI. You can completely override this default by explicitly setting the `UNICORE__CREDENTIAL_OFFER_URI` variable or the `credential_offer_uri` config value.

In most setups, the default value is recommended and usually the best choice.

:::

| Environment variable            | `config.yaml`          |
| ------------------------------- | ---------------------- |
| `UNICORE__CREDENTIAL_OFFER_URI` | `credential_offer_uri` |

#### Example

```yaml
credential_offer_uri: https://my-domain.example.test/credential-offer
```

### Request URI

The URI used to represent a request object, such as in OpenID Connect flows. This is used to pass request parameters by reference.

:::note

This variable is **optional**. By default, the `UNICORE__PUBLIC_URL` is used as the base, and the `/request` segment is appended to form the request URI. You can completely override this default by explicitly setting the `UNICORE__REQUEST_URI` variable or the `request_uri` config value.

In most setups, the default value is recommended and usually the best choice.

:::

| Environment variable   | `config.yaml` |
| ---------------------- | ------------- |
| `UNICORE__REQUEST_URI` | `request_uri` |

#### Example

```yaml
request_uri: https://my-domain.example.test/request
```

### Redirect URI

The URI to which the client will be redirected after completing an authorization or credential flow. This must be registered and accessible by the client.

:::note

This variable is **optional**. By default, the `UNICORE__PUBLIC_URL` is used as the base, and the `/redirect` segment is appended to form the redirect URI. You can completely override this default by explicitly setting the `UNICORE__REDIRECT_URI` variable or the `redirect_uri` config value.

In most setups, the default value is recommended and usually the best choice.

:::

| Environment variable    | `config.yaml`  |
| ----------------------- | -------------- |
| `UNICORE__REDIRECT_URI` | `redirect_uri` |

#### Example

```yaml
redirect_uri: https://my-client.example.test/callback
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

- `mongodb` _(default)_
- `postgres`
- `in_memory`

##### `connection_string`

Only required when `type` is `postgres`.

#### Example

```yaml
event_store:
  type: postgres
  connection_string: postgresql://user:password@database:5432/demo
```

## Additional Configuration Options

### CORS
Enable CORS (permissive) for browser-based access.

| Environment variable  | `config.yaml` |
| --------------------- | ------------- |
| `UNICORE__CORS_ENABLED` | `cors_enabled` |

Default: `false`

### Domain Linkage
Enable domain linkage (only works with `did:web`).

| Environment variable              | `config.yaml`         |
| --------------------------------- | --------------------- |
| `UNICORE__DOMAIN_LINKAGE_ENABLED` | `domain_linkage_enabled` |

### External Server Response Timeout
The timeout for external server responses (in milliseconds).

| Environment variable                         | `config.yaml`                        |
| -------------------------------------------- | ------------------------------------ |
| `UNICORE__EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS` | `external_server_response_timeout_ms` |

Default: `1000`

## Look and Feel

:::info

Setting display values is currently not supported through environment variables. Please refer to `config.yaml`.

:::
