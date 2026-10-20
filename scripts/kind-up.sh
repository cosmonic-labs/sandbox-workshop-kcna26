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
# Let host pods pull from the local registry by container name.
docker network connect kind "$REGISTRY_CONTAINER" 2>/dev/null || true
helm upgrade --install wasmcloud "$ROOT/.upstream/charts/runtime-operator" \
  --namespace wasmcloud --create-namespace -f "$ROOT/k8s/values.yaml" --wait --timeout 10m
kubectl -n wasmcloud get pods
