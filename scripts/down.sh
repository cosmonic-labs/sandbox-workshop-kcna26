#!/usr/bin/env bash
# Stop everything up.sh and up-local.sh started. Pass --all to also delete the
# kind cluster, the registry container, the zot registry's storage and the
# NATS data (keyvalue buckets).
set -uo pipefail
source "$(dirname "$0")/env.sh"
for p in host nats registry; do
  [ -f "$RUN_DIR/$p.pid" ] && kill "$(cat "$RUN_DIR/$p.pid")" 2>/dev/null
  rm -f "$RUN_DIR/$p.pid"
done
if [ "${1:-}" = "--all" ]; then
  kind delete cluster --name "$CLUSTER" 2>/dev/null
  docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1
  rm -rf "$RUN_DIR/jetstream" "$RUN_DIR/registry"
fi
echo stopped
