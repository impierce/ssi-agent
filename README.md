# SSI Agent

[![semantic-release: angular](https://img.shields.io/badge/semantic--release-angular-e10079?logo=semantic-release)](https://github.com/angular/angular/blob/main/contributing-docs/commit-message-guidelines.md)
[![GitHub License](https://img.shields.io/github/license/impierce/ssi-agent)](https://github.com/impierce/ssi-agent/blob/HEAD/LICENSE)
[![Docker Pulls](https://img.shields.io/docker/pulls/impiercetechnologies/ssi-agent)](https://hub.docker.com/r/impiercetechnologies/ssi-agent)
[![twelve-factor-app](https://img.shields.io/badge/factors-twelve-blue)](https://12factor.net)

<!-- The "Twelve-Factor App" badge is a playful reference to the conventions we try to follow. -->

![Check licenses](https://github.com/impierce/ssi-agent/actions/workflows/check-licenses.yaml/badge.svg)

---

## Documentation

The full documentation is available [here](https://docs.impierce.com/unicore/).

The Beta version of the documentation is available [here](https://beta.docs.impierce.com/unicore/).

## API specification

[Follow these instructions](./agent_api_http/README.md) to inspect the HTTP API.

## Build & Run

Build and run the **SSI Agent** in a local Docker environment following [these steps](./agent_application/docker/README.md).

## Configuration

All configuration options are documented [here](./agent_application/CONFIGURATION.md).

## Breaking changes

From time to time breaking changes can occur. Please make sure you read the [CHANGELOG](./CHANGELOG.md) before updating.

<!-- TODO: add an updated overview the Architecture and Sequence Diagrams -->

## Releases

This project uses [semantic-release](https://semantic-release.gitbook.io) - plain and simple, without noteworthy custom configuration.

### Branches

| Branch name | Description                                                                                                  | Example tag      |
| ----------- | ------------------------------------------------------------------------------------------------------------ | ---------------- |
| `main`      | Current stable releases. Default version when pulling the `latest` Docker image.                             | `v1.2.1`         |
| `next`      | Upcoming major version (containing breaking changes). Can be considered a stable preview of coming features. | `v2.0.8`         |
| `beta`      | Pre-releases that are fully implemented, but require testing, validation and feedback.                       | `v2.0.8-beta.2`  |
| `alpha`     | Experimental early-stage testing and development.                                                            | `v2.1.2-alpha.4` |

### Merging strategy

All PRs to any of the branches defined above are squashed to preserve a clean history. Since the PR title is used as the commit message, it is important to follow a conventional commit style in order to allow semantic releases (next version is determined by the commits since the last version). Therefore, the PR title is automatically linted by a GitHub Action.
