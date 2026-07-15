############################################################################################
####  SERVER
############################################################################################

# `rust-musl-builder` rather than the official toolchain: it yields a static
# binary, so the runtime image only needs ffmpeg and CA certs.
FROM clux/muslrust:stable AS chef
USER root
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY ./xenovrastream .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json
# Build application
COPY ./xenovrastream .
RUN cargo build --target x86_64-unknown-linux-musl --release

############################################################################################
####  RUNNING
############################################################################################

# Not `scratch` like the drive: transcoding shells out to ffmpeg/ffprobe, so the
# runtime needs a real userland.
FROM debian:stable-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/xenovrastream /xenovrastream
# The UI is plain static files — no bundler step to run.
COPY ./ui /ui

# Uploads, ffmpeg scratch space and the segment cache all live here. Mount a
# volume: the cache is rebuildable, but losing an upload mid-transcode is not.
ENV WORK_DIR=/var/lib/xenovrastream
VOLUME ["/var/lib/xenovrastream"]

ENTRYPOINT ["/xenovrastream"]
