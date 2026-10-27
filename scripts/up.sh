#!/usr/bin/env bash
# Start the workshop cluster:
#   kind              a one-node Kubernetes cluster in Docker (context kind-workshop-toolbox)
#   registry:2        an OCI registry container on the kind network
#   runtime-operator  the upstream wasmCloud chart, v2.11.0: operator, NATS, one host pod
# then push the prebuilt components (or SRC=<dir>) and forward 127.0.0.1:18180
# to the host pod, so every step's curl command works unchanged.
#
# kind adds the context to your kubeconfig and switches to it, as it always
# does. `kubectl config use-context <yours>` switches back.
set -euo pipefail
source "$(dirname "$0")/env.sh"
CTX="kind-$CLUSTER"
K() { kubectl --context "$CTX" "$@"; }

# The notes folder (step 3), and a scratch copy for the read-write control
# case, reach the host pod through the kind node: a node path, mounted into
# the host container by k8s/values.yaml, and allowed by its hostPaths list.
mkdir -p "$RUN_DIR/notes-rw"
if ! kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  cat >"$RUN_DIR/kind-config.yaml" <<YAML
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
    extraMounts:
      - hostPath: $ROOT/steps/03-notes/notes
        containerPath: /workshop/notes
      - hostPath: $RUN_DIR/notes-rw
        containerPath: /workshop/notes-rw
YAML
  kind create cluster --name "$CLUSTER" --config "$RUN_DIR/kind-config.yaml"
fi

# The upstream chart, from the v2.11.0 tag (gitignored, fetched once).
if [ "$(git -C "$ROOT/.upstream" describe --tags 2>/dev/null)" != "v$WASMCLOUD_VERSION" ]; then
  rm -rf "$ROOT/.upstream"
  git clone --quiet --depth 1 --branch "v$WASMCLOUD_VERSION" https://github.com/wasmCloud/wasmCloud "$ROOT/.upstream"
fi

# The host pod pulls components from this container by its name on the kind
# network; the step manifests name it as $REGISTRY_CONTAINER:5000.
if ! docker ps --format '{{.Names}}' | grep -qx "$REGISTRY_CONTAINER"; then
  docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1 || true
  docker run -d --name "$REGISTRY_CONTAINER" -p "127.0.0.1:$KIND_REGISTRY_PORT:5000" registry:2 >/dev/null
  sleep 1
fi
docker network connect kind "$REGISTRY_CONTAINER" 2>/dev/null || true
REGISTRY="localhost:$KIND_REGISTRY_PORT" "$ROOT/scripts/push.sh" "${SRC:-$ROOT/artifacts}"

helm --kube-context "$CTX" upgrade --install wasmcloud "$ROOT/.upstream/charts/runtime-operator" \
  --namespace wasmcloud --create-namespace -f "$ROOT/k8s/values.yaml" --wait --timeout 10m
K -n wasmcloud rollout status deploy/hostgroup-default --timeout 5m >/dev/null

# 127.0.0.1:18180 -> the host pod's HTTP port. Restarted on every `make up`.
[ -f "$RUN_DIR/pf.pid" ] && kill "$(cat "$RUN_DIR/pf.pid")" 2>/dev/null || true
K -n wasmcloud port-forward deploy/hostgroup-default "${HOST_HTTP##*:}:80" --address "${HOST_HTTP%:*}" \
  >"$RUN_DIR/pf.log" 2>&1 &
echo $! >"$RUN_DIR/pf.pid"
for _ in $(seq 1 20); do nc -z "${HOST_HTTP%:*}" "${HOST_HTTP##*:}" 2>/dev/null && break; sleep 0.5; done

K -n wasmcloud get pods
echo
echo "cluster   kubectl context $CTX"
echo "registry  $REGISTRY_CONTAINER:5000 inside the cluster (127.0.0.1:$KIND_REGISTRY_PORT from here)"
echo "ingress   http://$HOST_HTTP  (use curl with a Host: header)"
