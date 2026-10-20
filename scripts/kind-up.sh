#!/usr/bin/env bash
# Kubernetes variant: kind + the upstream runtime-operator chart (v2.10.3).
# Uses its own kubeconfig in .run/ so your current kubectl context is untouched.
set -euo pipefail
source "$(dirname "$0")/env.sh"
export KUBECONFIG="$RUN_DIR/kubeconfig"
CLUSTER=workshop-toolbox
if ! kind get clusters | grep -qx "$CLUSTER"; then
  cat >"$RUN_DIR/kind-config.yaml" <<YAML
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
    extraMounts:
      - hostPath: $ROOT/steps/03-notes/notes
        containerPath: /workshop/notes
YAML
  kind create cluster --name "$CLUSTER" --config "$RUN_DIR/kind-config.yaml" --kubeconfig "$KUBECONFIG"
fi
# The upstream chart, from the v2.10.3 tag (gitignored, fetched once).
if [ ! -d "$ROOT/.upstream/charts/runtime-operator" ]; then
  git clone --quiet --depth 1 --branch v2.10.3 https://github.com/wasmCloud/wasmCloud "$ROOT/.upstream"
fi
# The cluster pulls components from a registry:2 container on the kind
# network, by container name (scripts/kube-apply.sh rewrites the image
# references). Reuse it if `REGISTRY_MODE=docker make up` already started it;
# otherwise start one on 127.0.0.1:15052 so it never collides with zot.
if ! docker ps --format '{{.Names}}' | grep -qx "$REGISTRY_CONTAINER"; then
  docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1 || true
  docker run -d --name "$REGISTRY_CONTAINER" -p "127.0.0.1:15052:5000" registry:2 >/dev/null
  sleep 1
fi
docker network connect kind "$REGISTRY_CONTAINER" 2>/dev/null || true
KIND_REGISTRY="localhost:$(docker port "$REGISTRY_CONTAINER" 5000/tcp | head -1 | sed 's/.*://')"
REGISTRY="$KIND_REGISTRY" "$ROOT/scripts/push.sh" "${SRC:-$ROOT/artifacts}"
helm upgrade --install wasmcloud "$ROOT/.upstream/charts/runtime-operator" \
  --namespace wasmcloud --create-namespace -f "$ROOT/k8s/values.yaml" --wait --timeout 10m
kubectl -n wasmcloud get pods
