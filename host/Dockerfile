# syntax=docker/dockerfile:1
#
# The workshop's host: wash v2.12.0, built with the host-component-plugins
# feature like the upstream all-features image, plus the toolbox host plugin
# (host/toolbox) compiled in. scripts/build-host.sh prepares the context (the
# upstream source with host/wash-v2.12.0.patch applied and the plugin crate
# beside it) and runs this.
#
# The same stages as upstream's Dockerfile, with cargo's registry and target
# folders kept between builds, so a change to the plugin rebuilds in minutes.

FROM cgr.dev/chainguard/rust:latest-dev AS builder
WORKDIR /src
ENV RUST_BACKTRACE=1
USER root
RUN apk update && apk add --no-cache protoc protobuf protobuf-dev
COPY . .
ARG CARGO_BUILD_JOBS=6
# One target folder per architecture, so an amd64 build and an arm64 build
# don't overwrite each other's.
ARG TARGETARCH
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,id=workshop-host-target-${TARGETARCH},target=/src/target \
    CARGO_HOME=/root/.cargo CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS} \
    cargo build --release --bin wash --features host-component-plugins \
    && cp target/release/wash /usr/local/bin/wash

FROM cgr.dev/chainguard/wolfi-base
# No org.opencontainers.image.source label: it would link the package to
# the (private) workshop repo, which keeps the package from being made public.
LABEL org.opencontainers.image.description="wasmCloud wash v2.12.0 with the workshop toolbox host plugin"
RUN apk update && apk add --no-cache git
COPY --from=builder /usr/local/bin/wash /usr/local/bin/wash
# Fails here, not in the cluster, if the two stages' libc differ.
RUN ["/usr/local/bin/wash", "--version"]
ENTRYPOINT ["/usr/local/bin/wash"]
