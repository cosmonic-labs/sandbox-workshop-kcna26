# The one place artifacts/ is built (scripts/artifacts.sh), locally and in CI.
#
# A component's bytes depend on the build machine's host triple: Cargo hashes
# it into every crate that has a build script, and symbol names, function
# order and data layout follow from those hashes. So every build of record
# happens here, on linux/arm64: native on Apple silicon, emulated elsewhere,
# and GitHub's ubuntu-24.04-arm runners in CI. The image and wasi-sdk are
# pinned by digest.
FROM --platform=linux/arm64 docker.io/library/rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e

RUN rustup target add wasm32-wasip2

# wasi-sdk 28: clang and the wasi sysroot for sqlite's bundled SQLite.
ARG WASI_SDK_SHA256=72ef6f605308936151772ee70fa0bf8dca7742b554abbcd6ed63b802ae21b288
RUN curl -fsSL -o /tmp/wasi-sdk.tar.gz \
      https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-28/wasi-sdk-28.0-arm64-linux.tar.gz \
  && echo "$WASI_SDK_SHA256  /tmp/wasi-sdk.tar.gz" | sha256sum -c - \
  && tar xzf /tmp/wasi-sdk.tar.gz -C /opt \
  && mv /opt/wasi-sdk-28.0-arm64-linux /opt/wasi-sdk \
  && rm /tmp/wasi-sdk.tar.gz
ENV WASI_SDK_PATH=/opt/wasi-sdk

# wac 0.12.0: composes the talks MCP server with Cosmonic's MCP bridge
# (scripts/compose-mcp.sh).
ARG WAC_SHA256=80fe01c4cbf1e59526069d46f51a8a396be6f8df09ecc060d949181a317c08f5
RUN curl -fsSL -o /usr/local/bin/wac \
      https://github.com/bytecodealliance/wac/releases/download/v0.12.0/wac-cli-aarch64-unknown-linux-musl \
  && echo "$WAC_SHA256  /usr/local/bin/wac" | sha256sum -c - \
  && chmod +x /usr/local/bin/wac
