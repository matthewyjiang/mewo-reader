# syntax=docker/dockerfile:1
# Build from the repository root. Rust matches rust-toolchain.toml; SQLite is bundled.
FROM node:22-bookworm-slim AS web-builder
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
# Release tags arrive as MEWO_VERSION with the leading "v" removed.
# Empty falls back to web/package.json and a null commit.
ARG MEWO_VERSION=
ARG MEWO_COMMIT=
ENV MEWO_VERSION=$MEWO_VERSION \
    MEWO_COMMIT=$MEWO_COMMIT
RUN npm run build

FROM rust:1.88.0-bookworm AS builder
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock server/rust-toolchain.toml ./
COPY server/src ./src
COPY server/migrations ./migrations
# Same build args as the web stage. option_env! reads them at compile time.
ARG MEWO_VERSION=
ARG MEWO_COMMIT=
ENV MEWO_VERSION=$MEWO_VERSION \
    MEWO_COMMIT=$MEWO_COMMIT
RUN cargo build --release --locked --bin mewo-server --jobs 8

FROM debian:bookworm-slim
RUN useradd --uid 10001 --user-group --home-dir /data --no-create-home mewo \
    && mkdir -p /data \
    && chown mewo:mewo /data

COPY --from=builder /src/target/release/mewo-server /usr/local/bin/mewo-server
COPY --from=web-builder /src/web/dist /srv/web
COPY server/THIRD_PARTY_LICENSES.md /usr/share/doc/mewo-server/THIRD_PARTY_LICENSES.md

USER mewo
ENV MEWO_DATA_DIR=/data \
    MEWO_LISTEN=0.0.0.0:8787 \
    MEWO_WEB_DIR=/srv/web

VOLUME ["/data"]
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/mewo-server"]
