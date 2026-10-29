# Workshop environment for GitHub Codespaces (or any devcontainer host).
# Everything the steps need is here, so `make doctor` passes on first boot:
# wash, kind, kubectl, helm, nats and wasm-tools (pinned and checksum
# verified by scripts/tools.sh), curl, nc, and
# Rust 1.98.1 with the wasm32-wasip2 target for building the tools yourself.
# Docker comes from the docker-in-docker feature (devcontainer.json), so
# `make up` runs the kind cluster inside the codespace.
FROM mcr.microsoft.com/devcontainers/base:2-ubuntu-24.04

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates curl make netcat-openbsd unzip \
 && rm -rf /var/lib/apt/lists/*

# The pinned tools go in /opt/workshop-tools. postCreateCommand links them into
# the repo's .tools/, where the scripts and make doctor look for them.
COPY scripts/tools.sh /tmp/tools.sh
RUN TOOLS_DIR=/opt/workshop-tools bash /tmp/tools.sh \
 && rm -rf /opt/workshop-tools/dl /tmp/tools.sh \
 && ln -s /opt/workshop-tools/* /usr/local/bin/

# Rust, only for "build it yourself". Set INSTALL_RUST=false to skip it and
# save about 1 GB and a few minutes of image build.
ARG INSTALL_RUST=true
ARG RUST_VERSION=1.98.1
USER vscode
RUN if [ "$INSTALL_RUST" = true ]; then \
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain "$RUST_VERSION" --target wasm32-wasip2; \
    fi
ENV PATH="/home/vscode/.cargo/bin:${PATH}"
USER root
