# Application Profiles

UniCore can run with different application profiles. An application profile includes configuration defaults, changes runtime behavior and can enforce certain restrictions.
Currently, UniCore supports the following application profiles:

### Production _(default)_

If nothing is further specified, UniCore will run in production mode by default.

### Development

Can be enabled by setting the environment variable `UNICORE__PROFILE=development`.

This profile is designed to be used for development purposes. It requires less initial configuration, enables a more verbose API and debugging information.

Development-only settings can be configured under the `dev` section. These values are ignored when UniCore runs with the `production` profile.

| Environment variable                  | `config.yaml`              |
| ------------------------------------- | -------------------------- |
| `UNICORE__DEV__API_KEY`               | `dev.api_key`              |
| `UNICORE__DEV__LOG_SENSITIVE_VALUES`  | `dev.log_sensitive_values` |

`dev.log_sensitive_values` (default `false`) adds the values that are redacted otherwise to the `debug` log lines: access
and refresh tokens, authorization and pre-authorized codes, credential offers and their URIs, received credentials,
pushed authorization and notification requests, recipient email addresses, and the full `detail` of rejected and failed
requests. They only appear with `RUST_LOG` at `debug` for the module that logs them, and UniCore logs a warning at
startup while the setting is enabled. Do not enable it where logs are collected or shared.

#### Example

```yaml
dev:
  api_key: 123123123
  log_sensitive_values: true
```
