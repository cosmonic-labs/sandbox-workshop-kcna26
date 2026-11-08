#!/usr/bin/env bash
# Nothing destructive by default. Pass --all to delete the kind cluster
# and the registry container.
set -uo pipefail
source "$(dirname "$0")/env.sh"
if [ "${1:-}" = "--all" ]; then
  kind delete cluster --name "$CLUSTER" 2>/dev/null
  docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1
  echo "deleted the cluster and the registry"
  # kind leaves no current context; go back to the one make up replaced.
  if [ -s "$RUN_DIR/previous-context" ]; then
    prev=$(cat "$RUN_DIR/previous-context")
    kubectl config use-context "$prev" >/dev/null 2>&1 \
      && echo "kubectl context is $prev again"
    rm -f "$RUN_DIR/previous-context"
  fi
else
  echo "nothing to stop; make clean deletes the cluster and the registry"
fi
