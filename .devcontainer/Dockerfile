# Workshop environment for GitHub Codespaces (or any devcontainer host).
# Everything the steps need is here, so `make doctor` passes on first boot:
# wash, kind, kubectl, helm, nats and wasm-tools (pinned and checksum
# verified by scripts/tools.sh), curl, nc, and
# Rust 1.98.1 with the wasm32-wasip2 target for building the tools yourself,
# and wasi-sdk for the C inside tools/sqlite.
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

# wasi-sdk, for tools/sqlite: SQLite is C, so building it for WebAssembly needs
# a C compiler that targets WASI. Pinned and checked like scripts/tools.sh.
ARG WASI_SDK_VERSION=33.0
RUN if [ "$INSTALL_RUST" = true ]; then \
      case "$(uname -m)" in \
        x86_64)  A=x86_64; SHA=0ba8b5bfaeb2adf3f29bab5841d76cf5318ab8e1642ea195f88baba1abd47bce ;; \
        aarch64) A=arm64;  SHA=4f98ee738c7abb45c81a94d1461fc53cc569d1cd01498951c8184d841a027844 ;; \
        *) echo "no wasi-sdk for $(uname -m)"; exit 1 ;; \
      esac; \
      F="wasi-sdk-${WASI_SDK_VERSION}-${A}-linux.tar.gz"; \
      curl -fsSL -o "/tmp/$F" "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_SDK_VERSION%%.*}/$F" \
      && echo "$SHA  /tmp/$F" | sha256sum -c - \
      && mkdir -p /opt/wasi-sdk \
      && tar -xzf "/tmp/$F" -C /opt/wasi-sdk --strip-components=1 \
      && rm "/tmp/$F"; \
    fi
# Target-specific names, so only builds for wasm32-wasip2 see them.
ENV WASI_SDK_PATH=/opt/wasi-sdk \
    CC_wasm32_wasip2=/opt/wasi-sdk/bin/clang \
    AR_wasm32_wasip2=/opt/wasi-sdk/bin/llvm-ar \
    CFLAGS_wasm32_wasip2=--sysroot=/opt/wasi-sdk/share/wasi-sysroot
