#!/usr/bin/env bash
# Download the pinned workshop tools into .tools/ and check every download
# against a SHA256 recorded here. Nothing is installed outside this folder,
# and nothing runs with sudo.
#
#   wash         v2.10.3   wasmCloud CLI and host (digests from the release's
#                          sigstore attestation, wash.sigstore.json)
#   nats-server  v2.14.7   message bus plus key value and object storage
#   nats         v0.5.0    NATS command line client
#   zot          v2.1.22   small single binary OCI registry (replaces Docker)
#   wasm-tools   v1.258.3  inspects component imports (step 1, make prove)
#
# Already present and matching? It is skipped. Pass --force to re-download.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="$ROOT/.tools"
DL="$TOOLS/dl"
FORCE=0; [ "${1:-}" = "--force" ] && FORCE=1

WASH_VERSION=2.10.3
NATS_SERVER_VERSION=2.14.7
NATS_CLI_VERSION=0.5.0
ZOT_VERSION=2.1.22
WASM_TOOLS_VERSION=1.258.3

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)              PLAT=darwin-arm64 ;;
  Darwin-x86_64)             PLAT=darwin-amd64 ;;
  Linux-x86_64|Linux-amd64)  PLAT=linux-amd64 ;;
  Linux-aarch64|Linux-arm64) PLAT=linux-arm64 ;;
  *)
    echo "This machine ($(uname -s) $(uname -m)) is not supported directly."
    echo "Use WSL2 (Windows) or GitHub Codespaces (Code > Codespaces > Create)."
    exit 1 ;;
esac

# file name in the release, sha256, for this platform
case "$PLAT" in
  darwin-arm64)
    WASH_ASSET=wash-aarch64-apple-darwin
    WASH_SHA=4f1abb0e226ced4ccaff1fa1cbf5fbe510f698c07cf69ac48946c64165375f93
    NATS_SERVER_SHA=9fc225cddebc67438345ad988c5d6bfc6daad16a7609bcceaafa88bcab9b9001
    NATS_CLI_SHA=37a6f924bc42c72a38856d9c6125c38885732b2a13387b8bc15d1ff80987520b
    ZOT_SHA=c81a26040ae79c7e8a90c306a2377c69982a86ff35c259427bd9accea0eca14c
    WT_ASSET=aarch64-macos
    WT_SHA=dacd5e6a79c0ad513c369373eb0e595dafadae01fddbb47cda8dee543a31495d ;;
  darwin-amd64)
    WASH_ASSET=wash-x86_64-apple-darwin
    WASH_SHA=c77f02b7df4f2df92857b1fdfe4f62cb20a06b196ae02990031881796384ebc9
    NATS_SERVER_SHA=c35bd7c1b84ee0b2de25f7aa0a9f1bb4834640ac8a93698a6b79f9d1de3cf5ff
    NATS_CLI_SHA=9fcf6dbe6462c78a1564d03c8970158f50f50c31ebda5350d2da4a597de796e7
    ZOT_SHA=3b18f445a8229372b0b521c07945dfd18a0a8ec1745601a22974f554f7614fb9
    WT_ASSET=x86_64-macos
    WT_SHA=c0d8452e2e564d414cc1c7273c0cc5c8013d3de1ccc7650deea87eeda1ad3f26 ;;
  linux-amd64)
    WASH_ASSET=wash-x86_64-unknown-linux-musl
    WASH_SHA=a8aaac68f02b506f669789cdd8cdcf5e4633443a57e32445bf7ff2cc47b687f6
    NATS_SERVER_SHA=e5c20b1cb2c0566b54c544312e91e011f9e130c5c80f16a14f4cf28ef30b8be2
    NATS_CLI_SHA=d9fc93e9e9ab0310deff7e719a8a0da6f9d66f26d473f039aaee70155507aecc
    ZOT_SHA=a053169f97b4ceaa051e054d53971799188a398cb36acbaf5e08622ec8664c44
    WT_ASSET=x86_64-linux
    WT_SHA=99b486a9ddc19af0260cd5f98f3b709a60817a3acd20efc9c582d719caebbf6d ;;
  linux-arm64)
    WASH_ASSET=wash-aarch64-unknown-linux-musl
    WASH_SHA=87605a88428f84b33be1b3c33d510585bb0c38af9818369b1c1fc5a5ad32acdf
    NATS_SERVER_SHA=50e0258a1b7290f3fe2e4f17506b74a90ffb1dd77867ae3a402eb0c9e2b0d40f
    NATS_CLI_SHA=39496b0e332ba8d1b7dcbac2019c7d1e7a7f0d0ccefef9da547e9d6eeed408c8
    ZOT_SHA=4224c23122c43638f92875d6fb4f23564472d2584a348cb057440bcf4b43bf08
    WT_ASSET=aarch64-linux
    WT_SHA=4b9d10d460343bf7e13dcfa8ddc1e06661ff1894d4e3caf3920780c5b21dc1dd ;;
esac

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# fetch <url> <file in .tools/dl> <sha256>
fetch() {
  local url=$1 out="$DL/$2" want=$3
  if [ -f "$out" ] && [ "$(sha256 "$out")" = "$want" ]; then return 0; fi
  echo "  downloading $url"
  curl -fsSL --retry 3 -o "$out.part" "$url"
  local got; got=$(sha256 "$out.part")
  if [ "$got" != "$want" ]; then
    rm -f "$out.part"
    echo "CHECKSUM MISMATCH for $2" >&2
    echo "  expected $want" >&2
    echo "  got      $got" >&2
    echo "The download was deleted. Try again; if it repeats, do not use this file." >&2
    exit 1
  fi
  mv "$out.part" "$out"
}

unzip_to() { # zip, destination dir
  if command -v unzip >/dev/null 2>&1; then unzip -qo "$1" -d "$2"
  else python3 -m zipfile -e "$1" "$2"; fi
}

have() { # tool name, version string the tool prints
  [ "$FORCE" = 0 ] && [ -x "$TOOLS/$1" ] && "$TOOLS/$1" --version 2>/dev/null | grep -qF "$2"
}

mkdir -p "$DL"
echo "Platform: $PLAT. Tools go in $TOOLS"

if have wash "wash $WASH_VERSION"; then echo "ok  wash $WASH_VERSION (already there)"; else
  fetch "https://github.com/wasmCloud/wasmCloud/releases/download/v$WASH_VERSION/$WASH_ASSET" \
    "$WASH_ASSET-$WASH_VERSION" "$WASH_SHA"
  install -m 0755 "$DL/$WASH_ASSET-$WASH_VERSION" "$TOOLS/wash"
  echo "ok  wash $WASH_VERSION"
fi

if have nats-server "v$NATS_SERVER_VERSION"; then echo "ok  nats-server $NATS_SERVER_VERSION (already there)"; else
  f="nats-server-v$NATS_SERVER_VERSION-$PLAT"
  fetch "https://github.com/nats-io/nats-server/releases/download/v$NATS_SERVER_VERSION/$f.tar.gz" \
    "$f.tar.gz" "$NATS_SERVER_SHA"
  tar -xzf "$DL/$f.tar.gz" -C "$DL" "$f/nats-server"
  install -m 0755 "$DL/$f/nats-server" "$TOOLS/nats-server"
  echo "ok  nats-server $NATS_SERVER_VERSION"
fi

if have nats "$NATS_CLI_VERSION"; then echo "ok  nats $NATS_CLI_VERSION (already there)"; else
  f="nats-$NATS_CLI_VERSION-$PLAT"
  fetch "https://github.com/nats-io/natscli/releases/download/v$NATS_CLI_VERSION/$f.zip" \
    "$f.zip" "$NATS_CLI_SHA"
  unzip_to "$DL/$f.zip" "$DL"
  install -m 0755 "$DL/$f/nats" "$TOOLS/nats"
  echo "ok  nats $NATS_CLI_VERSION"
fi

if have zot "v$ZOT_VERSION"; then echo "ok  zot $ZOT_VERSION (already there)"; else
  f="zot-$PLAT-minimal"
  fetch "https://github.com/project-zot/zot/releases/download/v$ZOT_VERSION/$f" \
    "$f-$ZOT_VERSION" "$ZOT_SHA"
  install -m 0755 "$DL/$f-$ZOT_VERSION" "$TOOLS/zot"
  echo "ok  zot $ZOT_VERSION"
fi

if have wasm-tools "$WASM_TOOLS_VERSION"; then echo "ok  wasm-tools $WASM_TOOLS_VERSION (already there)"; else
  f="wasm-tools-$WASM_TOOLS_VERSION-$WT_ASSET"
  fetch "https://github.com/bytecodealliance/wasm-tools/releases/download/v$WASM_TOOLS_VERSION/$f.tar.gz" \
    "$f.tar.gz" "$WT_SHA"
  tar -xzf "$DL/$f.tar.gz" -C "$DL" "$f/wasm-tools"
  install -m 0755 "$DL/$f/wasm-tools" "$TOOLS/wasm-tools"
  echo "ok  wasm-tools $WASM_TOOLS_VERSION"
fi

echo
echo "Done. The scripts find these tools on their own. To use nats or wash by hand in"
echo "this terminal, run:  export PATH=\"$TOOLS:\$PATH\""
