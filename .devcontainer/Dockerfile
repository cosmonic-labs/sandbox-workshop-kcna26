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
# save about 1 GB and a few minutes of image build. rustup-init is a pinned
# release, checked against a SHA256 recorded here before it runs (the same
# files static.rust-lang.org publishes as rustup-init.sha256).
ARG INSTALL_RUST=true
ARG RUST_VERSION=1.98.1
ARG RUSTUP_VERSION=1.28.2
ARG RUSTUP_SHA256_AMD64=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c
ARG RUSTUP_SHA256_ARM64=e3853c5a252fca15252d07cb23a1bdd9377a8c6f3efa01531109281ae47f841c
USER vscode
RUN if [ "$INSTALL_RUST" = true ]; then \
      set -eu; \
      case "$(uname -m)" in \
        x86_64)  triple=x86_64-unknown-linux-gnu;  sum="$RUSTUP_SHA256_AMD64" ;; \
        aarch64) triple=aarch64-unknown-linux-gnu; sum="$RUSTUP_SHA256_ARM64" ;; \
        *) echo "unsupported architecture $(uname -m)" >&2; exit 1 ;; \
      esac; \
      curl --proto '=https' --tlsv1.2 -fsSL -o /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/$triple/rustup-init"; \
      echo "$sum  /tmp/rustup-init" | sha256sum -c -; \
      chmod +x /tmp/rustup-init; \
      /tmp/rustup-init -y --profile minimal --default-toolchain "$RUST_VERSION" --target wasm32-wasip2; \
      rm /tmp/rustup-init; \
    fi
ENV PATH="/home/vscode/.cargo/bin:${PATH}"
USER root
