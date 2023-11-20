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
RUN cargo build --release --bin ssi-agent

FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/ssi-agent /usr/local/bin
ENTRYPOINT ["/usr/local/bin/ssi-agent"]
