# Logging

UniCore logs through [`tracing`](https://docs.rs/tracing). The log format is set with `log_format` (see
[Configuration](../configuration/CONFIGURATION.md)) and the level with `RUST_LOG`, which defaults to `info`. Logs are
read by operators, shipped to log backends and kept for a long time. So a log line must be useful to someone running
UniCore, and safe to hand to someone who has no access to the data UniCore holds.

No linter can tell whether a log line leaks data or is at the right level. These rules are applied in review, by human
and agent reviewers alike.

## Levels

| Level   | Use it for                                                                          | Examples                                                       |
| ------- | ----------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `error` | A server-side failure that someone has to act on                                    | A store is unreachable, a response cannot be built             |
| `warn`  | Something went wrong but was handled, and may need attention if it repeats          | A remote document is malformed and skipped, a fallback is used |
| `info`  | Lifecycle and rare state changes worth seeing in production; never once per request | Startup, the redacted configuration, shutdown                  |
| `debug` | Steps of a request or command, for tracing a single flow                            | "Executing command", "Event committed", "Request completed"    |

- `info` is the default level, so every `info` line is paid for in production. A line that fires on every request
  or command belongs at `debug`.
- An error the caller is told about isn't logged as an error on top. A command rejected by a business rule is a
  client error, logged at `debug` (`Command rejected`). The HTTP layer logs client errors at `info` and server errors
  at `error`, both by the problem type and title.

## Never log, at any level

`debug` logs get switched on in production to chase a problem, so this applies to `debug` too.

- **Secrets:** keys, passwords, API keys, tokens (access and refresh tokens, authorization and pre-authorized codes),
  and credentials in URLs (userinfo, query values).
- **Credentials and presentations:** issued credentials, VP tokens and their decoded contents, ID tokens, and signed
  JWTs in general.
- **Personal data:** email addresses, names, profile picture URLs, and any claim about a holder.
- **Request and response bodies and headers.**
- **`Debug` dumps of commands, events, views or aggregate state.** These hold all of the above, and are large.
- **Raw request paths.** Use the route template (`MatchedPath`), since a path can carry client input.

These are fine: identifiers (aggregate IDs, DIDs and Entity Identifiers of organisations), types and operation names,
counts, sequence numbers, hosts, status codes and durations.

Offer IDs are an exception for now. An offer ID is part of the offer's `credential_offer_uri` and is its `issuer_state`,
so it gives access to the offer ([#508](https://github.com/impierce/ssi-agent/issues/508)). Don't add new log lines
with one.

When a value has to be shown, show its shape rather than its content, for example `count = items.len()` or
`target_host = url.host_str()`.

### Development only: `dev.log_sensitive_values`

To debug a flow locally, a value from the list above may be added to a `debug` line through `sensitive` or
`sensitive_debug` from `agent_shared::sensitive_logging`:

```rust
debug!(
    path = location.split('?').next(),
    location = sensitive(&location),
    "Redirecting after consent"
);
```

The field is left out of the event unless `dev.log_sensitive_values` is enabled, which only the `development` profile
allows (see [Profiles](../configuration/profiles.md)). This is the only way such a value may reach a log line, and only
at `debug`.

## Structured fields

Prefer fields over interpolation. Fields can be filtered and indexed, and they keep the message constant:

```rust
// Avoid
info!("Sending credential offer to: {}", target);
// Prefer
debug!(target_host = target_url.host_str(), "Sending credential offer");
```

Use `%` (`Display`) for identifiers. A `?` (`Debug`) field on a domain type is almost always a dump of its content.

## Commands and events

- **Aggregates don't log the commands they handle or their state.** The command handler logs each command once at
  `debug`, by its operation name.
- **`apply` doesn't log.** It runs on every replay of an aggregate, not only when an event is new, so a line there
  repeats the aggregate's whole history on every command.
- **Committed events are logged once**, at `debug`, by aggregate ID, sequence and event type, by `SimpleLoggingQuery`
  in `agent_store`. Payloads are not logged.

## What already protects you

- `sensitive_headers()` in `agent_api_http` marks `Authorization`, `Proxy-Authorization`, `Cookie` and `Set-Cookie`
  as sensitive wherever headers are printed. `app` applies it as the outermost layer.
- `with_request_logging` in `agent_api_http` gives each request a span named after its route template, and logs one
  line per response (`log_response`) without headers or bodies. It logs the detail of a server error separately at
  `debug`, with its URLs redacted. With `dev.log_sensitive_values`, that detail is logged in full, and so is the detail
  of a client error. `app` applies it to the built-in routes; apply it to custom routes as well.
- The SSE events router is merged after `with_request_logging`, so event payloads are not logged.
- Secret configuration fields are serialized as `<REDACTED>` (`#[serde(serialize_with = "redact")]`), and
  `ApplicationConfiguration::redacted()` also redacts URL credentials and webhook paths. Log the configuration only
  through `redacted()`.
- With OpenTelemetry export active, JSON log lines written within a span carry its `trace_id` and `span_id`, so a
  request is easier to follow by correlating its lines than by logging more of them.

When adding a secret to the configuration, mark it with `redact`. When adding code that handles one of the types listed
under [Never log](#never-log-at-any-level), check what the surrounding log lines print.
