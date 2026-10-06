# syntax=docker/dockerfile:1
FROM rust:1.98.1-slim-bookworm AS chef
# The process shutdown integration test invokes the external `kill` utility.
RUN apt-get update && apt-get install -y --no-install-recommends procps \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.77
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
# Cargo resolves the workspace, but the server does not compile Bevy or need assets.
COPY crates/client/Cargo.toml ./crates/client/Cargo.toml
COPY crates/launcher/Cargo.toml ./crates/launcher/Cargo.toml
COPY crates/core ./crates/core
COPY crates/server ./crates/server
# Keep workspace metadata valid for current and historical client manifests.
RUN mkdir -p crates/client/src crates/launcher/src \
    && touch crates/client/src/lib.rs crates/client/src/main.rs \
             crates/launcher/src/lib.rs crates/launcher/src/main.rs \
    && cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /build/recipe.json ./recipe.json
# These layers depend on manifests/lockfile, so source edits keep both profiles.
# All stages use the same Rust version and /build path for fingerprint reuse.
RUN cargo chef cook --locked --tests -p rubblekin_core -p rubblekin_server --recipe-path recipe.json \
    && cargo chef cook --locked --release -p rubblekin_server --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates/client/Cargo.toml ./crates/client/Cargo.toml
COPY crates/launcher/Cargo.toml ./crates/launcher/Cargo.toml
COPY crates/core ./crates/core
COPY crates/server ./crates/server
RUN cargo test --locked -p rubblekin_core -p rubblekin_server \
    && cargo build --locked --release -p rubblekin_server

FROM debian:bookworm-slim
LABEL org.opencontainers.image.title="Rubblekin server" \
      org.opencontainers.image.description="Authoritative multiplayer Rubblekin test server" \
      org.opencontainers.image.source="https://github.com/maccam912/rubblekin"
RUN groupadd --gid 10001 rubblekin && useradd --uid 10001 --gid rubblekin --create-home rubblekin \
    && mkdir /data && chown rubblekin:rubblekin /data
COPY --from=build /build/target/release/rubblekin-server /usr/local/bin/rubblekin-server
USER 10001:10001
WORKDIR /data
EXPOSE 7878/tcp
STOPSIGNAL SIGTERM
ENTRYPOINT ["rubblekin-server"]
CMD ["--bind", "0.0.0.0:7878", "--save", "/data/world.json"]
