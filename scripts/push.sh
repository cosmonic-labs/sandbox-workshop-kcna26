#!/usr/bin/env bash
# Push components to the cluster's registry.
#
#   scripts/push.sh          every prebuilt artifacts/*.wasm (or SRC=<dir>), as
#                            $REGISTRY/workshop/<name>:$TAG
#   scripts/push.sh --mine   your own curl build (make build), as
#                            $REGISTRY/workshop/tool-curl:mine
#
# Your build gets its own tag, so it never replaces the prebuilt tool that
# steps 1 and 3 to 6 use. steps/02-build-curl/workload-mine.yaml runs it.
set -euo pipefail
source "$(dirname "$0")/env.sh"

push() { # file, image reference
  "$WASH" oci push --insecure "$2" "$1" >/dev/null
  echo "pushed $2"
}

if [ "${1:-}" = "--mine" ]; then
  f="$ROOT/target/wasm32-wasip2/release/tool_curl.wasm"
  if [ ! -f "$f" ]; then
    echo "no build at $f: run make build first" >&2
    exit 1
  fi
  push "$f" "$REGISTRY/workshop/tool-curl:mine"
  exit 0
fi

SRC="${1:-$ROOT/artifacts}"
for f in "$SRC"/*.wasm; do
  name=$(basename "$f" .wasm | tr _ -)
  push "$f" "$REGISTRY/workshop/$name:$TAG"
done
