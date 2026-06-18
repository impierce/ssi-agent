# Copilot Cloud Agent Onboarding

## Repository Overview

**ssi-agent** is a Self-Sovereign Identity (SSI) agent implementing OpenID4VCI (issuance) and OpenID4VP (verification) protocols. It's a Rust Tokio-based service that manages identity credentials, digital wallets, and credential verification flows.

- **Language**: Rust 1.76.0+ (enforced in Cargo.toml `rust-version`)
- **Build System**: Cargo workspace with 18 crates
- **HTTP Framework**: Axum 0.8 with Tokio async runtime
- **Persistence**: PostgreSQL with CQRS-ES event sourcing pattern
- **Code Size**: ~15K LOC across distributed domain-driven crates
- **Architecture**: Domain-driven design with bounded contexts (issuance, holder, verification, authorization, identity)

## Build & Test Instructions

All commands run from repository root. **Always use `cargo` commands; do not use shell scripts for building.**

### Bootstrap (one-time setup)

```bash
# Install Rust 1.76.0+ (use dtolnay/rust-toolchain@stable for CI version)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy

# Verify installation
cargo --version  # Should show 1.76.0+
```

### Build

```bash
# Full workspace build (includes all 18 crates)
cargo build --workspace

# Build specific crate
cargo build -p agent_api_http

# Build with optimizations
cargo build --release
```

**Expected time**: ~45 seconds for clean full build (first time may be 2+ minutes with git dependency downloads).

### Format Check

```bash
# Check code formatting (max width 120 chars per rustfmt.toml)
cargo fmt --all -- --check

# Auto-fix formatting
cargo fmt --all
```

### Lint

```bash
# Run clippy with pedantic warnings as errors
cargo clippy --all-targets --all-features -- -D warnings
```

### Test

```bash
# Run all tests (serial_test synchronizes certain tests to prevent race conditions)
cargo test --workspace

# Run tests for single crate
cargo test -p agent_issuance

# Run specific test with output
cargo test -p agent_issuance test_verify_credential_response -- --nocapture
```

**Critical constraint**: Test `test_verify_credential_response` in `agent_issuance` requires 32 MiB stack. It's already wrapped with `run_with_large_stack` helper; do NOT remove or modify this wrapper. The test cannot use `#[future(awt)]` rstest fixtures because rstest evaluates fixtures on the normal stack before the large-stack wrapper executes.

### Dependency & Security Checks

```bash
# Audit dependencies for vulnerabilities
cargo install cargo-audit
cargo audit

# Check license compliance (runs weekly in CI)
cargo install cargo-deny
cargo deny check licenses --hide-inclusion-graph
```

### Code Coverage (local)

```bash
# Generate LCOV coverage report
cargo install cargo-llvm-cov
cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
```

## Project Layout

### Root Directory Structure

- **agent\_\*** crates: 14 bounded contexts (api_http, application, holder, issuance, verification, etc.)
- **shared-kernel/**: Core DDD abstractions (ApplicationService, CommandHandlerFactory, ViewRepository)
- **infrastructure/**: Persistence adapters (MongoDB, PostgreSQL) and authorization adapters
- **.github/workflows/**: CI/CD pipelines (format-lint-test, audit, coverage, docker, release)
- **agent_application/**: Entry point (main.rs runs the Tokio async runtime)
- **agent_application/docker/**: Docker Compose setup with PostgreSQL + pgAdmin + optional Prometheus/Grafana

### Key Configuration Files

| File                                    | Purpose                                                                                                        |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `Cargo.toml`                            | Workspace definition, workspace dependencies, Clippy pedantic lints, patch.crates-io (sd-jwt hasher trait fix) |
| `rustfmt.toml`                          | max_width = 120                                                                                                |
| `deny.toml`                             | Dependency graph and license checks                                                                            |
| `.releaserc.yaml`                       | Semantic-release config (squash-merge PRs, conventional commits)                                               |
| `commitlint.config.mjs`                 | PR title validation enforces conventional commit style                                                         |
| `agent_application/docker/compose.yaml` | PostgreSQL 15, pgAdmin, Prometheus/Grafana (optional)                                                          |

### Critical Workflow Files

- **.github/workflows/format-lint-test.yaml**: Runs `cargo fmt --check`, `cargo clippy`, `cargo test --workspace` on all PRs
- **.github/workflows/audit.yaml**: Daily dependency audit via cargo-audit
- **.github/workflows/check-licenses.yaml**: Weekly license check via cargo-deny
- **.github/workflows/coverage.yaml**: Generates LCOV coverage, uploads to Codecov on main/next/beta/alpha
- **.github/workflows/build-push-docker.yaml**: Multi-platform Docker builds (amd64/arm64)

## Architecture & Key Constraints

### Domain-Driven Design Bounded Contexts

- **agent_issuance**: OpenID4VCI credential issuance flows
- **agent_holder**: Credential storage and management
- **agent_verification**: Credential verification via OpenID4VP
- **agent_authorization**: OAuth 2.0 authorization flows
- **agent_identity**: DID document management and resolution
- **agent_api_http**: Axum HTTP API handlers (all endpoints routed through this crate)
- **shared-kernel**: Command/Query service, CQRS patterns, dependency injection

### Testing Patterns

- **Test Framework**: rstest with `#[rstest]` and `#[future(awt)]` fixtures for async tests
- **Test Synchronization**: Tests marked with `#[serial_test::serial]` when they share HTTP event-publisher state
- **Stack-Intensive Tests**: Tests requiring 32 MiB stack are wrapped with `run_with_large_stack<F>` helper thread spawning
- **Fixture Setup**: Manual fixture helpers (not rstest parameters) must be called inside large-stack wrappers to avoid premature stack overflow

### External Dependencies (Git Pinned)

```
siopv2, oid4vci, oid4vc-core, oid4vc-manager, oid4vp → https://github.com/impierce/openid4vc (rev 0a5090e)
identity_* crates → https://github.com/iotaledger/identity (v1.9.6-beta.1)
iota-sdk → https://github.com/iotaledger/iota (specific rev)
oauth_tsl → https://github.com/impierce/oauth-token-status-list (rev 0aa0228)
sd-jwt (patched) → https://github.com/impierce/sd-jwt-payload (rev f28789a, Hasher trait Send+Sync fix)
```

### Configuration & Environment

- **Config file**: `agent_application/example.config.yaml` (copied to required locations)
- **Environment vars**: Prefixed with `UNICORE__` (e.g., `UNICORE__APPLICATION_URL`, `UNICORE__LOG_FORMAT`)
- **Required for testing**: `.env` file optional; Docker Compose auto-manages Postgres credentials (demo_user/demo_pass)
- **Stronghold secret manager**: Generates `stronghold.dat` at runtime; controlled via `UNICORE__SECRET_MANAGER__STRONGHOLD_PATH`

## Pre-commit Validation (Always Trust CI, Validate Locally First)

Run locally before pushing to avoid PR rejection:

```bash
# Full CI validation sequence (runs in format-lint-test.yaml)
cargo fmt --all -- --check && \
  cargo clippy --all-targets --all-features -- -D warnings && \
  cargo test --workspace && \
  git diff --exit-code

# If git diff fails, code generation files changed (check agent_api_http openapi-generated.yaml)
```

**If tests timeout or panic**: Check for stack-related issues in test output (search for "stack" or "overflow"). All known stack-intensive tests are already wrapped; report new cases.

## Trust This Document

Cloud agents should use these instructions as the authoritative source for building and testing this repository. Only perform additional file searches if:

1. Build/test steps documented here fail with clear error messages not addressed above
2. Changes are needed to crate-specific logic (then search agent\_\*/src/ for relevant modules)
3. New CI validations are added to .github/workflows/ (then update this document)

All other exploration wastes cycles. Use this guide to complete tasks efficiently.
