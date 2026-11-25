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

Test categories are determined by their dependency footprint, not only by where their source file lives.

### 1. Hermetic Unit Tests

- Unit tests must operate in memory using mock or fake adapters and must not perform disk I/O, mutate global configuration, or depend on external services.
- Logic that currently reads global configuration should expose a function that accepts the relevant configuration explicitly, leaving the global lookup in a thin production adapter.
- Controlled local fakes such as an in-process HTTP mock are acceptable, but tests that exercise file-backed cryptography or mutable process-wide state are component tests.

### 2. Isolated Component Tests

Component tests may exercise existing global-configuration or file-backed infrastructure seams when replacing those seams would turn a coverage change into an unrelated production refactor. Such tests must:

- run in an external test target when process isolation is needed;
- resolve fixtures with `concat!(env!("CARGO_MANIFEST_DIR"), ...)` or use a unique temporary path, never a CWD-relative path;
- initialize file-backed fixtures once per test process or give each test its own fixture;
- serialize access to mutable process-wide state and restore the original state after every test; and
- remain independent of test order and external network services.

These allowances document containment of legacy seams, not approval to introduce new global state into production code.

### 3. Container Integration Tests

Tests that verify databases, message brokers, telemetry collectors, or other external infrastructure belong in feature-gated external test targets. Each test target runs in its own process and owns the containers and configuration it mutates.

### 4. Explicit Dependency Injection

- New domain and application services should accept configuration explicitly rather than calling global static getters like `agent_shared::config::config()`.
- Existing global-configuration seams may be covered by isolated component tests until they are replaced as part of a dedicated architectural change.

---

## Rationale

- **Robustness**: Prevents test suite failures caused by static `once_cell` poisoning and CWD changes across workspaces.
- **Speed & Parallelism**: Keeps unit tests parallel while making the smaller number of stateful component tests explicit and safely isolated.
- **Clean Layering**: Maintains a clear boundary between pure domain/application logic and infrastructure/I/O concerns.
