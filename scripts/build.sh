#!/usr/bin/env bash
# Build components for wasm32-wasip2, release, reproducibly: the same source
# gives the same bytes wherever this checkout is, on machines of one host
# triple. scripts/artifacts.sh runs it in the pinned build image
# (build.Dockerfile), so CI can rebuild artifacts/ and check it. Compilers
# write absolute paths into a binary (panic locations, C asserts), so each is
# mapped to a fixed name: this checkout to /src, CARGO_HOME to /cargo, the
# Rust toolchain to /rust and wasi-sdk to /wasi-sdk.
#
#   scripts/build.sh                 every component (sqlite needs wasi-sdk)
#   scripts/build.sh -p tool-curl    only the packages named
#
# sqlite's bundled SQLite is C: set WASI_SDK_PATH to a wasi-sdk 28 install
# (CI uses the same version), and its clang and llvm-ar are used.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT"

mkdir -p "${CARGO_HOME:-$HOME/.cargo}"
cargo_home="$(cd "${CARGO_HOME:-$HOME/.cargo}" && pwd -P)"
sysroot="$(cd "$(rustc --print sysroot)" && pwd -P)"
export RUSTFLAGS="--remap-path-prefix=$ROOT=/src --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$sysroot=/rust"
if [ -n "${WASI_SDK_PATH:-}" ]; then
  sdk="$(cd "$WASI_SDK_PATH" && pwd -P)"
  export CC_wasm32_wasip2="${CC_wasm32_wasip2:-$sdk/bin/clang}"
  export AR_wasm32_wasip2="${AR_wasm32_wasip2:-$sdk/bin/llvm-ar}"
  export CFLAGS_wasm32_wasip2="-ffile-prefix-map=$ROOT=/src -ffile-prefix-map=$cargo_home=/cargo -ffile-prefix-map=$sdk=/wasi-sdk"
fi
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
exec cargo build --locked --release --target wasm32-wasip2 "$@"
