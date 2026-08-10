# Three-stage build: bundle the frontend, compile a release binary, then
# copy both into a slim Debian image with just the SQLite + TLS runtime
# libs we link against.

# UPGRADE_POINT
FROM node:24-bookworm-slim AS frontend

WORKDIR /web

# Deps first so a source-only change doesn't re-run the install.
COPY web/package.json web/package-lock.json ./
RUN npm ci

COPY web/ ./
# build.mjs writes to ../static/dist, i.e. /static/dist in this stage,
# together with the manifest.json the server reads to resolve the hashed
# filenames.
RUN npm run build

# UPGRADE_POINT
FROM rust:1.90-slim-bookworm AS build

WORKDIR /build

# Install only the system deps we need to link (sqlx links libsqlite3
# dynamically only if we enable the bundled feature; we don't here, so
# pull in the headers). Build-essential is the rust toolchain's gcc.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libsqlite3-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache deps. Copy manifests, build a dummy main, then drop in the real
# source so cargo only recompiles iggybilly itself on source-only changes.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs \
    && cargo build --locked --release \
    && rm -rf src target/release/deps/iggybilly* target/release/iggybilly*

COPY src ./src
COPY migrations ./migrations
# Askama compiles the page shell at build time (the #[template(path =
# ...)] macro reads templates/ relative to the crate root), so it must be
# present before `cargo build`.
COPY templates ./templates
RUN cargo build --locked --release

# UPGRADE_POINT
FROM debian:bookworm-slim AS app

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    libsqlite3-0 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /build/target/release/iggybilly /app/iggybilly
# Bundles + manifest.json, served at /static. The binary reads
# static/dist/manifest.json at startup, relative to this WORKDIR.
COPY --from=frontend /static ./static

ENV IGGYBILLY_DATA_DIR=/var/lib/iggybilly
ENV IGGYBILLY_LISTEN_ADDR=0.0.0.0:9020
EXPOSE 9020

ENTRYPOINT ["/app/iggybilly"]
CMD ["serve"]
