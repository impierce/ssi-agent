# https://github.com/LukeMathWalker/cargo-chef

FROM rust:1 AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .

RUN cargo build --release --bin agent_application

FROM debian:bookworm-slim AS runtime
COPY --from=builder /app/target/release/agent_application /usr/local/bin/
WORKDIR /app

COPY ../../agent_application/config.yml agent_application/config.yml
COPY ../../agent_secret_manager/tests/res/selv.stronghold res/stronghold
COPY ../../agent_event_publisher_http/config.yml agent_event_publisher_http/config.yml
COPY ../../agent_verification/presentation_definitions agent_verification/presentation_definitions

ENTRYPOINT ["/usr/local/bin/agent_application"]
