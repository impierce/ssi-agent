# ADR 0003: Hermetic Test Architecture and Configuration Decoupling

**Status**: Accepted  
**Date**: 2026-07-30  
**Context**: Establishing guidelines for hermetic, thread-safe unit and integration testing without global static state or CWD-dependent file I/O.

---

## Context

The application test suite previously relied heavily on global static configuration (`agent_shared::config::CONFIG`) and disk-based test fixtures (`test.config.yaml` and `test.stronghold.dat`).

This created several operational and architectural challenges:

1. **Working Directory Sensitivity**: Test configurations and secret manager binaries relied on relative paths (e.g. `../agent_shared/tests/test.config.yaml` and `../agent_secret_manager/tests/res/test.stronghold.dat`). When running tests from outer workspace roots or nested bounded contexts, the current working directory (CWD) differed, causing file lookup failures (`Config file not found: ./config.yaml`).
2. **Static Cell Poisoning**: Initializing `CONFIG` via `once_cell::sync::Lazy` would panic if environment variables or default config files were missing. Once a `Lazy` initializer panics, `once_cell` poisons the static instance, causing every subsequent test in the process that accesses `config()` to panic with `Lazy instance has previously been poisoned`.
3. **Parallel Test Race Conditions**: Concurrent test threads calling `Subject::test_subject().await` ran against a shared `./stronghold.dat` file on disk. This led to file lock contention and password mismatch panics (`InvalidPassword`).
4. **Coupling Application Logic to Infrastructure**: Unit tests for application-layer components (such as `PublicVerificationContextBuilder`) implicitly invoked `Subject::test_subject().await`, forcing pure domain/application unit tests to perform crypto disk I/O and configuration parsing.

---

## Decision

We have established the following standards and path resolution rules for the codebase:

### 1. Compile-Time Manifest Path Resolution (Short-Term Compatibility)
Where test fixtures or configuration files must be referenced from source, paths must be resolved at compile-time using `concat!(env!("CARGO_MANIFEST_DIR"), ...)` rather than CWD-relative strings (`../...`). This guarantees that tests resolve identical file paths whether executed from `ssi-agent`, outer workspace roots, or any subcrate directory.

### 2. Hermetic & In-Memory Unit Testing (Target Architecture)
- **Unit tests** must operate strictly in memory using mock/fake adapters (e.g., `InMemoryStore`, `MockSubject`) and must not perform disk I/O or trigger global configuration loading.
- Unit tests for builders and application services must test validation and logic branches directly without instantiating heavy infrastructure components.

### 3. Explicit Dependency Injection
- Domain and application services should accept configuration explicitly rather than calling global static getters like `agent_shared::config::config()`.

---

## Rationale

- **Robustness**: Prevents test suite failures caused by static `once_cell` poisoning and CWD changes across workspaces.
- **Speed & Parallelism**: Eliminates disk contention, allowing tests to run in parallel without file locks or password collisions.
- **Clean Layering**: Maintains a clear boundary between pure domain/application logic and infrastructure/I/O concerns.
