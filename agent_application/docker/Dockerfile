# https://github.com/LukeMathWalker/cargo-chef

FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG APP_VERSION
ARG GIT_COMMIT_HASH
ARG DOCKER_BUILD_TIMESTAMP
ARG APP_RELEASE_CHANNEL

ENV APP_VERSION=${APP_VERSION}
ENV GIT_COMMIT_HASH=${GIT_COMMIT_HASH}
ENV DOCKER_BUILD_TIMESTAMP=${DOCKER_BUILD_TIMESTAMP}
ENV APP_RELEASE_CHANNEL=${APP_RELEASE_CHANNEL}

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin agent_application

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/agent_application /usr/local/bin/

ENTRYPOINT ["/usr/local/bin/agent_application"]
