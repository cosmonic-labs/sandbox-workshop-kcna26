#!/usr/bin/env bash
# artifacts/ holds the prebuilt components make up pushes, so the workshop
# needs no Rust. They are this repo's source built by scripts/build.sh inside
# the pinned build image (build.Dockerfile), which makes them reproducible:
# CI rebuilds them the same way on every push and pull request, fails when a
# committed file differs from its own build, and publishes its build from
# main to the repo's packages.
#
#   scripts/artifacts.sh check    build every component and compare each with
#                                 artifacts/SHA256SUMS; changes nothing
#   scripts/artifacts.sh update   build every component, copy each into
#                                 artifacts/ and rewrite artifacts/SHA256SUMS
#
# Both need Docker: the image has Rust and wasi-sdk. check also needs wash
# (make tools), to confirm the plugins' digests pinned in k8s/values.yaml.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

COMPONENTS="agent github_api guide mcp_talks_toolset mock_sites model_scripted tool_curl tool_gh tool_sqlite"
# The build's own target and cargo home, apart from any local build's.
BUILD_DIR=target/linux-arm64
OUT=$BUILD_DIR/wasm32-wasip2/release
IMAGE=sandbox-workshop-build:local
if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi

# Cosmonic's MCP bridge, the composed build Cosmonic Desktop runs, published
# as a public image. The talks MCP toolset is composed with it
# (scripts/compose-mcp.sh); it is fetched by the digest of its one layer, so
# the toolset is always composed with the same bytes.
BRIDGE_REPO=cosmonic/agent-tools/mcp-bridge-composed
BRIDGE_LAYER=sha256:caa18c6db8618c4d82efce9f78db9e33e17e786abd243d811697eba251be659f
BRIDGE=$BUILD_DIR/mcp_bridge_composed.wasm

fetch_bridge() {
  if [ -f "$BRIDGE" ] && [ "sha256:$($SUM "$BRIDGE" | cut -d' ' -f1)" = "$BRIDGE_LAYER" ]; then
    return
  fi
  local token
  token=$(curl -fsS "https://ghcr.io/token?scope=repository:$BRIDGE_REPO:pull" \
    | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')
  curl -fsSL -H "Authorization: Bearer $token" -o "$BRIDGE.part" \
    "https://ghcr.io/v2/$BRIDGE_REPO/blobs/$BRIDGE_LAYER"
  if [ "sha256:$($SUM "$BRIDGE.part" | cut -d' ' -f1)" != "$BRIDGE_LAYER" ]; then
    echo "ghcr.io/$BRIDGE_REPO: layer does not match $BRIDGE_LAYER" >&2
    rm -f "$BRIDGE.part"
    return 1
  fi
  mv "$BRIDGE.part" "$BRIDGE"
}

build() {
  docker build -q --platform linux/arm64 -t "$IMAGE" - <build.Dockerfile >/dev/null
  mkdir -p "$BUILD_DIR"
  fetch_bridge
  docker run --rm --platform linux/arm64 --user "$(id -u):$(id -g)" \
    -e HOME=/tmp -e CARGO_HOME="/src/$BUILD_DIR/cargo" -e CARGO_TARGET_DIR="/src/$BUILD_DIR" \
    -e CARGO_BUILD_JOBS \
    -v "$ROOT:/src" -w /src "$IMAGE" \
    sh -c "scripts/build.sh && scripts/compose-mcp.sh $OUT $BRIDGE"
}

sums() { # the SHA256SUMS lines for the build
  for c in $COMPONENTS; do
    echo "$($SUM "$OUT/$c.wasm" | cut -d' ' -f1)  artifacts/$c.wasm"
  done | sort -k2
}

# The image digest each plugin is pinned to in k8s/values.yaml must be its
# build's: push the build to a throwaway registry and compare. Needs wash
# (make tools).
check_plugin_pins() {
  local registry=workshop-artifacts-check port=15077 out id repo want got bad=0
  docker rm -f "$registry" >/dev/null 2>&1 || true
  docker run -d --rm --name "$registry" -p "127.0.0.1:$port:5000" \
    "$(bash -c 'source scripts/env.sh; echo "$REGISTRY_IMAGE"')" >/dev/null
  for _ in $(seq 1 40); do curl -fs "http://localhost:$port/v2/" >/dev/null && break; sleep 0.5; done
  out=$(REGISTRY="localhost:$port" scripts/push.sh "$OUT") || { docker rm -f "$registry" >/dev/null; return 1; }
  docker rm -f "$registry" >/dev/null
  for id in github-api:github-api; do
    repo=${id#*:}
    want=$(awk -v id="${id%%:*}" '$0 ~ "- id: "id"$" { on = 1 } on && /digest:/ { print $2; exit }' k8s/values.yaml)
    got=$(echo "$out" | sed -n "s#.*/$repo:.* (\(sha256:[0-9a-f]*\))#\1#p")
    if [ "$want" != "$got" ]; then
      echo "k8s/values.yaml pins plugin ${id%%:*} to $want; this build is $got" >&2
      bad=1
    fi
  done
  [ "$bad" = 0 ] && echo "k8s/values.yaml pins each plugin to this build's digest"
  return "$bad"
}

case "${1:-}" in
  check)
    build
    want=$(sort -k2 artifacts/SHA256SUMS)
    got=$(sums)
    if [ "$want" != "$got" ]; then
      echo "artifacts/ does not match a build of this source:" >&2
      diff <(echo "$want") <(echo "$got") | sed -n 's/^< /  committed: /p; s/^> /  built:     /p' >&2
      echo "Run make artifacts and commit the result." >&2
      exit 1
    fi
    echo "artifacts/ matches a build of this source ($(echo "$got" | wc -l | tr -d ' ') components)"
    check_plugin_pins
    ;;
  update)
    build
    for c in $COMPONENTS; do cp "$OUT/$c.wasm" "artifacts/$c.wasm"; done
    sums >artifacts/SHA256SUMS
    echo "artifacts/ and artifacts/SHA256SUMS updated from this build"
    ;;
  *)
    echo "usage: scripts/artifacts.sh check|update" >&2
    exit 2
    ;;
esac
