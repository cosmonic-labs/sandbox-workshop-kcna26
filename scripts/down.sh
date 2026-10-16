#!/usr/bin/env bash
# Stop everything up.sh and kind-up.sh started. Pass --all to also delete the
# kind cluster and the registry container.
set -uo pipefail
source "$(dirname "$0")/env.sh"
for p in host nats pf; do
  [ -f "$RUN_DIR/$p.pid" ] && kill "$(cat "$RUN_DIR/$p.pid")" 2>/dev/null
  rm -f "$RUN_DIR/$p.pid"
done
if [ "${1:-}" = "--all" ]; then
  kind delete cluster --name workshop-toolbox --kubeconfig "$RUN_DIR/kubeconfig" 2>/dev/null
  docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1
  rm -rf "$RUN_DIR/jetstream"
fi
echo stopped
