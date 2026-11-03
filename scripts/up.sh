#!/usr/bin/env bash
# Start the workshop cluster:
#   kind              a one-node Kubernetes cluster in Docker (context kind-workshop-toolbox)
#   registry:2        an OCI registry container on the kind network
#   runtime-operator  the upstream wasmCloud chart, v2.11.0: operator, NATS and
#                     two host groups (default for the agents, fixtures for the
#                     toy sites), one host pod each
#   data-nats         the data service behind keyvalue (k8s/data-nats.yaml)
#   workshop-sites    the toy sites' namespace and Services (k8s/sites.yaml)
#   agent Service     the way in to the agent (k8s/agent-service.yaml)
# then push the prebuilt components (or SRC=<dir>). 127.0.0.1:18180 reaches the
# host pods through a small ingress (k8s/ingress.yaml) that kind maps to your
# laptop and that picks the host group by Host header, so every step's curl
# command works unchanged.
#
# kind adds the context to your kubeconfig and switches to it, as it always
# does. `kubectl config use-context <yours>` switches back.
set -euo pipefail
source "$(dirname "$0")/env.sh"
CTX="kind-$CLUSTER"
K() { kubectl --context "$CTX" "$@"; }

# The notes folder (step 6), and a scratch copy for the read-write control
# case, reach the host pod through the kind node: a node path, mounted into
# the host container by k8s/values.yaml, and allowed by its hostPaths list.
mkdir -p "$RUN_DIR/notes-rw"
if ! kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  cat >"$RUN_DIR/kind-config.yaml" <<YAML
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
    extraPortMappings:
      - containerPort: 30180
        hostPort: ${HOST_HTTP##*:}
        listenAddress: "${HOST_HTTP%:*}"
      - containerPort: 30422
        hostPort: ${DATA_NATS_URL##*:}
        listenAddress: "127.0.0.1"
    extraMounts:
      - hostPath: $ROOT/steps/06-notes/notes
        containerPath: /workshop/notes
      - hostPath: $RUN_DIR/notes-rw
        containerPath: /workshop/notes-rw
YAML
  kind create cluster --name "$CLUSTER" --config "$RUN_DIR/kind-config.yaml"
fi

# The upstream chart, from the v2.11.0 tag (gitignored, fetched once). The
# check compares commits: the same commit carries more than one tag, so
# `git describe` can name another one.
TAG_REF="v$WASMCLOUD_VERSION"
if [ "$(git -C "$ROOT/.upstream" rev-parse -q --verify HEAD 2>/dev/null)" != \
     "$(git -C "$ROOT/.upstream" rev-parse -q --verify "$TAG_REF^{commit}" 2>/dev/null || echo none)" ]; then
  rm -rf "$ROOT/.upstream"
  git init -q "$ROOT/.upstream"
  git -C "$ROOT/.upstream" fetch --quiet --depth 1 https://github.com/wasmCloud/wasmCloud \
    "refs/tags/$TAG_REF:refs/tags/$TAG_REF"
  git -C "$ROOT/.upstream" -c advice.detachedHead=false checkout --quiet "$TAG_REF^{commit}"
fi

# The host pod pulls components from this container by its name on the kind
# network; the step manifests name it as $REGISTRY_CONTAINER:5000. It restarts
# with Docker, so a codespace or laptop restart does not strand the host pods
# with nothing to pull from.
if ! docker ps --format '{{.Names}}' | grep -qx "$REGISTRY_CONTAINER"; then
  docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1 || true
  docker run -d --restart unless-stopped --name "$REGISTRY_CONTAINER" -p "127.0.0.1:$KIND_REGISTRY_PORT:5000" registry:2 >/dev/null
  sleep 1
fi
docker network connect kind "$REGISTRY_CONTAINER" 2>/dev/null || true
REGISTRY="localhost:$KIND_REGISTRY_PORT" "$ROOT/scripts/push.sh" "${SRC:-$ROOT/artifacts}"

K apply -f "$ROOT/k8s/data-nats.yaml" >/dev/null
# The fixtures host group runs in workshop-sites, which must exist first.
K apply -f "$ROOT/k8s/sites.yaml" >/dev/null
helm --kube-context "$CTX" upgrade --install wasmcloud "$ROOT/.upstream/charts/runtime-operator" \
  --namespace wasmcloud --create-namespace -f "$ROOT/k8s/values.yaml" --wait --timeout 10m
K -n wasmcloud rollout status deploy/hostgroup-default --timeout 5m >/dev/null
K -n workshop-sites rollout status deploy/hostgroup-fixtures --timeout 5m >/dev/null

# The ingress image, and step 4's container image, are loaded into the node
# so nothing pulls at the workshop.
for img in docker.io/library/nginx:1.27.5-alpine busybox:1.36; do
  docker image inspect "$img" >/dev/null 2>&1 || docker pull -q "$img" >/dev/null
  kind load docker-image "$img" --name "$CLUSTER" >/dev/null 2>&1 || true
done
K apply -f "$ROOT/k8s/agent-service.yaml" >/dev/null
K apply -f "$ROOT/k8s/ingress.yaml" >/dev/null
K -n wasmcloud rollout status deploy/workshop-ingress --timeout 3m >/dev/null

# The platform provisions the stores; a workload can open a store but not
# create one. scripts/stores.sh lists them.
K -n workshop-data rollout status deploy/data-nats --timeout 3m >/dev/null
"$ROOT/scripts/stores.sh" create

# The toy sites (planted page, collector, fake model) run from the start, on
# the fixtures host group. A host pulls an image when a workload starts, and
# the tag above is the same on every run, so recreate the toy sites to pick up
# what was just pushed. The collector's count lives in its bucket and stays.
K -n workshop-sites delete workloaddeployment toy-sites --ignore-not-found --wait=true >/dev/null
K apply -f "$ROOT/steps/00-fixtures/toy-sites.yaml" >/dev/null
K -n workshop-sites wait --for=condition=Ready workloaddeployment/toy-sites --timeout=180s >/dev/null || true
for _ in $(seq 1 30); do
  curl -s -o /dev/null -m 2 "http://$HOST_HTTP/" && break; sleep 1
done

K get pods -n wasmcloud
K get pods -n workshop-sites
echo
echo "cluster   kubectl context $CTX"
echo "registry  $REGISTRY_CONTAINER:5000 inside the cluster (127.0.0.1:$KIND_REGISTRY_PORT from here)"
echo "ingress   http://$HOST_HTTP  (use curl with a Host: header)"
