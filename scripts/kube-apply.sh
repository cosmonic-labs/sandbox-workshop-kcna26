#!/usr/bin/env bash
# kubectl apply a step manifest on the kind cluster. Two rewrites only:
# images come from the registry's name on the kind network, and the relative
# notes path becomes the path mounted into the host pod.
set -euo pipefail
source "$(dirname "$0")/env.sh"
export KUBECONFIG="$RUN_DIR/kubeconfig"
verb=apply; [ "${1:-}" = "--delete" ] && { verb=delete; shift; }
for f in "$@"; do
  sed -e "s#localhost:15051/#$REGISTRY_CONTAINER:5000/#g" \
      -e "s#path: \.\./03-read-notes/notes#path: /workshop/notes#" \
      -e "s#path: \./notes#path: /workshop/notes#" "$f" | kubectl "$verb" -f -
done
