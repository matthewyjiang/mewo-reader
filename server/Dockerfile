# syntax=docker/dockerfile:1
# Hosted library API. Rust 1.88 matches rust-toolchain.toml.
# SQLite is bundled, so the runtime image does not need libsqlite3.

FROM rust:1.88.0-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY migrations ./migrations

RUN cargo build --release --locked --bin mewo-server --jobs 12

FROM debian:bookworm-slim
RUN useradd --uid 10001 --user-group --home-dir /data --no-create-home mewo \
    && mkdir -p /data \
    && chown mewo:mewo /data

COPY --from=builder /src/target/release/mewo-server /usr/local/bin/mewo-server

USER mewo
ENV MEWO_DATA_DIR=/data \
    MEWO_LISTEN=0.0.0.0:8787

VOLUME ["/data"]
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/mewo-server"]
