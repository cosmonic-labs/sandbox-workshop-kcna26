#!/usr/bin/env bash
# Download the pinned workshop tools into .tools/ and check every download
# against a SHA256 recorded here. Nothing is installed outside this folder,
# and nothing runs with sudo.
#
#   wash         v2.11.0   wasmCloud CLI; pushes the components (digests from the release's
#                          sigstore attestation, wash.sigstore.json)
#   wasm-tools   v1.258.3  inspects component imports (step 1, make prove)
#   kind         v0.33.0   a Kubernetes cluster in Docker (needs Docker)
#   kubectl      v1.37.1   the Kubernetes CLI
#   helm         v3.22.0   installs the upstream wasmCloud chart
#
# Already present and matching? It is skipped. Pass --force to re-download.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="${TOOLS_DIR:-$ROOT/.tools}"   # the devcontainer image sets TOOLS_DIR
DL="$TOOLS/dl"
FORCE=0; [ "${1:-}" = "--force" ] && FORCE=1

WASH_VERSION=2.11.0
WASM_TOOLS_VERSION=1.258.3
KIND_VERSION=0.33.0
KUBECTL_VERSION=1.37.1
HELM_VERSION=3.22.0

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
    WASH_SHA=365d896eaed3894f1c551a1ed7351c2aae9dfa5548ce82041ead92e6253d1954
    WT_ASSET=aarch64-macos
    WT_SHA=dacd5e6a79c0ad513c369373eb0e595dafadae01fddbb47cda8dee543a31495d
    KIND_SHA=0c8c7dbe5e23594a198b786c4bc13dacc101fa6196b0cb0b23a1ca44e61f4b4f
    KUBECTL_SHA=fd65982c97ddad3106754b69ffa196d0e543aa591930ae52aed1adfb92f8c77f
    HELM_SHA=4c9982a6cdeb458b60258df66b55398ca5b19293f6877faffe2909ad6f23dfe0 ;;
  darwin-amd64)
    WASH_ASSET=wash-x86_64-apple-darwin
    WASH_SHA=9c287ea9e027f0c2d391400e7897c8e8225109cfee6efaf2e7b238afea233f17
    WT_ASSET=x86_64-macos
    WT_SHA=c0d8452e2e564d414cc1c7273c0cc5c8013d3de1ccc7650deea87eeda1ad3f26
    KIND_SHA=5a99f26f57246dc9319dd294803313197a0f34d33c525b3ea8b655db5916ece0
    KUBECTL_SHA=6851381c486ff6edd691623e3d65c87cb9a5b02887ff8fbbb38d8a031b748387
    HELM_SHA=bd1d09f316558dda23698527859600936fed1371021ce0f2272d66b2fdcfa69c ;;
  linux-amd64)
    WASH_ASSET=wash-x86_64-unknown-linux-musl
    WASH_SHA=ba28e81516c3147519506e64b73d1351857088483deebcc6824e9b5d1fe72997
    WT_ASSET=x86_64-linux
    WT_SHA=99b486a9ddc19af0260cd5f98f3b709a60817a3acd20efc9c582d719caebbf6d
    KIND_SHA=aee6151561422756b764a4ae28e7f44cda5af5a9eead3cc9985112b1de8d8e0d
    KUBECTL_SHA=65691ff77eb6fa44c908b77a1082c9f092c3b9733b5cefabec0d1104890e21a8
    HELM_SHA=1e4ab49e429626cf6c6958d914248b78c9730803c2751b87627e171dc800e7bb ;;
  linux-arm64)
    WASH_ASSET=wash-aarch64-unknown-linux-musl
    WASH_SHA=af79cb91a8a5c2fa26576e70308fc3a080ca51f34ab74c5cc30816be483b6b9b
    WT_ASSET=aarch64-linux
    WT_SHA=4b9d10d460343bf7e13dcfa8ddc1e06661ff1894d4e3caf3920780c5b21dc1dd
    KIND_SHA=20022bee6cfcd5086cb7234d218e3454e6090022f2a8f55d1fa7fcf42c3867a2
    KUBECTL_SHA=ff749f4b78d9c4f1ec87307df9b50119ed819e2094aa9810cb9acffc3286c8c7
    HELM_SHA=f14e804dfee240f55525b667488fe9adca349e63e00c9af634c0beb1421ac310 ;;
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

if have wasm-tools "$WASM_TOOLS_VERSION"; then echo "ok  wasm-tools $WASM_TOOLS_VERSION (already there)"; else
  f="wasm-tools-$WASM_TOOLS_VERSION-$WT_ASSET"
  fetch "https://github.com/bytecodealliance/wasm-tools/releases/download/v$WASM_TOOLS_VERSION/$f.tar.gz" \
    "$f.tar.gz" "$WT_SHA"
  tar -xzf "$DL/$f.tar.gz" -C "$DL" "$f/wasm-tools"
  install -m 0755 "$DL/$f/wasm-tools" "$TOOLS/wasm-tools"
  echo "ok  wasm-tools $WASM_TOOLS_VERSION"
fi

if have kind "v$KIND_VERSION"; then echo "ok  kind $KIND_VERSION (already there)"; else
  fetch "https://github.com/kubernetes-sigs/kind/releases/download/v$KIND_VERSION/kind-$PLAT" \
    "kind-$PLAT-$KIND_VERSION" "$KIND_SHA"
  install -m 0755 "$DL/kind-$PLAT-$KIND_VERSION" "$TOOLS/kind"
  echo "ok  kind $KIND_VERSION"
fi

# kubectl prints its version with `kubectl version --client`, not --version.
if [ "$FORCE" = 0 ] && [ -x "$TOOLS/kubectl" ] && "$TOOLS/kubectl" version --client 2>/dev/null | grep -qF "v$KUBECTL_VERSION"; then
  echo "ok  kubectl $KUBECTL_VERSION (already there)"
else
  fetch "https://dl.k8s.io/release/v$KUBECTL_VERSION/bin/${PLAT%-*}/${PLAT#*-}/kubectl" \
    "kubectl-$PLAT-$KUBECTL_VERSION" "$KUBECTL_SHA"
  install -m 0755 "$DL/kubectl-$PLAT-$KUBECTL_VERSION" "$TOOLS/kubectl"
  echo "ok  kubectl $KUBECTL_VERSION"
fi

if [ "$FORCE" = 0 ] && [ -x "$TOOLS/helm" ] && "$TOOLS/helm" version --short 2>/dev/null | grep -qF "v$HELM_VERSION"; then
  echo "ok  helm $HELM_VERSION (already there)"
else
  f="helm-v$HELM_VERSION-$PLAT"
  fetch "https://get.helm.sh/$f.tar.gz" "$f.tar.gz" "$HELM_SHA"
  tar -xzf "$DL/$f.tar.gz" -C "$DL" "$PLAT/helm"
  install -m 0755 "$DL/$PLAT/helm" "$TOOLS/helm"
  echo "ok  helm $HELM_VERSION"
fi

echo
echo "Done. The scripts find these tools on their own. To use wash or kubectl by hand in"
echo "this terminal, run:  export PATH=\"$TOOLS:\$PATH\""
