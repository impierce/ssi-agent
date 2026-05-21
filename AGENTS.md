## Application design

- DDD: The application is designed around the concept of domain-driven design, where the core business logic is organized into domains and subdomains, with a clear separation of concerns.
- CQRS: The application uses the command-query responsibility segregation pattern, where commands are used to modify state and queries are used to read state. Events are treated as immutable after the are created. Projections are used to derive queryable read models from the event stream.
- Event sourcing: The application uses event sourcing to capture all changes to the state as a sequence of events. This allows for a complete audit trail and the ability to reconstruct past states by replaying events.
- Hexagonal architecture: The application tries to abstract how data comes in to the application and how it is stored.

### Architecture

Architecture Decision Records (ADRs) can be found in the [docs/adr](./docs/adr) directory.

## Standards and conventions

- The application follows the [Twelve-Factor App](https://12factor.net) methodology for building modern, scalable applications.
- The application uses semantic versioning for releases and follows a branching strategy to manage different stages of development (e.g. `main` for stable releases, `next` for upcoming major versions, `beta` for pre-releases, and `alpha` for experimental features).
- The application uses conventional commits for commit messages to enable automated semantic releases.
- For errors on the HTTP API, the application follows the [RFC 9457 Problem Details for HTTP APIs](https://www.rfc-editor.org/rfc/rfc9457.html) specification for error responses.
- Events follow the [CloudEvents specification (v1.0.2)](https://raw.githubusercontent.com/cloudevents/spec/refs/tags/v1.0.2/cloudevents/spec.md) for event structure and metadata _(this is not fully done as of today)_.

## Code style

- If you think a TODO is no longer relevant or has been addressed, ask the user if it can be removed. If it is still relevant but not being addressed in the current PR, leave it in place.
- Use comments only when necessary. Prefer self-explanatory code, but don't hesitate to add comments to clarify intent, especially for complex logic or non-obvious decisions.

### Cargo.toml

- Sort dependencies alphabetically by name, with the following exceptions:
  - Internal dependencies (using `path = "..."`) should be listed before external dependencies, followed by a blank line.
  - Every dependency from `https://github.com/impierce` should be listed before other dependencies, separated by a blank line.

### Commands

The following commands can be used to assert code quality:

- `cargo fmt --all` (format all code)
- `cargo clippy --all-targets --all-features -- -D warnings` (check for code lints, treat all warnings as errors)
- `cargo test generate_openapi_spec` (generate the OpenAPI specification by executing a test)
- `cargo test --workspace` (run all tests)
