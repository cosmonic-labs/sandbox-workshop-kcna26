#!/usr/bin/env bash
# Push every built component (or prebuilt artifacts/*.wasm) to the cluster's
# registry as $REGISTRY/workshop/<name>:$TAG.
set -euo pipefail
source "$(dirname "$0")/env.sh"
SRC="${1:-$ROOT/target/wasm32-wasip2/release}"
for f in "$SRC"/*.wasm; do
  name=$(basename "$f" .wasm | tr _ -)
  "$WASH" oci push --insecure "$REGISTRY/workshop/$name:$TAG" "$f" >/dev/null
  echo "pushed $REGISTRY/workshop/$name:$TAG"
done
