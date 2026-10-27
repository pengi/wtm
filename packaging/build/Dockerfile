# Build environment for the wtm packages: a static musl binary, packaged
# with cargo-deb and cargo-generate-rpm.
FROM rust:1-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends musl-tools \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo install --locked cargo-deb@3.7.0 cargo-generate-rpm@0.21.1 \
    && rm -rf /usr/local/cargo/registry
