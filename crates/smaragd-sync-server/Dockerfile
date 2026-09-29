# Smaragd sync server.
#
# Build from the REPOSITORY ROOT (the server path-depends on the shared protocol
# crate, so the build context must contain both):
#
#   docker build -f crates/smaragd-sync-server/Dockerfile -t smaragd-sync-server .
#
# Run:
#
#   TOKEN="$(openssl rand -hex 24)"; echo "$TOKEN"    # keep it: it creates vaults
#   docker run -d --name smaragd-sync -p 127.0.0.1:8080:8080 -v smaragd-sync-data:/data \
#       -e SMARAGD_SYNC_ADMIN_TOKEN="$TOKEN" smaragd-sync-server
#
# (localhost only: it speaks plain HTTP, so put a TLS reverse proxy in front for other
# machines — see the README)
#
# See crates/smaragd-sync-server/README.md for configuration and TLS.

FROM rust:1-slim-bookworm AS build
# rusqlite's bundled SQLite is compiled from C.
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY crates/smaragd-sync-protocol crates/smaragd-sync-protocol
COPY crates/smaragd-sync-server crates/smaragd-sync-server
WORKDIR /src/crates/smaragd-sync-server
# --locked: the committed Cargo.lock decides every dependency version.
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin smaragd \
 && mkdir /data \
 && chown smaragd /data
COPY --from=build /src/crates/smaragd-sync-server/target/release/smaragd-sync-server /usr/local/bin/smaragd-sync-server

ENV SMARAGD_SYNC_LISTEN_ADDR=0.0.0.0:8080 \
    SMARAGD_SYNC_DATA_DIR=/data
USER smaragd
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/smaragd-sync-server", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/smaragd-sync-server"]
