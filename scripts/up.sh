#!/usr/bin/env bash
# Start the workshop cluster:
#   kind              a one-node Kubernetes cluster in Docker (context kind-workshop-toolbox)
#   registry:2        an OCI registry container on the kind network
#   runtime-operator  the upstream wasmCloud chart, v2.12.0: operator, NATS and
#                     two host groups (default for the agents, mock-sites for the
#                     mock sites and the guide), one host pod each, on the
#                     workshop's host image (host/README.md)
#   mock-sites        the mock sites' namespace and Services (k8s/sites.yaml)
#   agent Service     the way in to the agent (k8s/agent-service.yaml)
#   guide             the steps as web pages (k8s/guide.yaml), on the mock-sites group
# then push the prebuilt components (or SRC=<dir>). 127.0.0.1:28180 reaches the
# host pods through a small ingress (k8s/ingress.yaml) that kind maps to your
# laptop and that picks the host group by Host header, so every step's curl
# command works unchanged.
#
# kind adds the context to your kubeconfig and switches to it, as it always
# does. make clean switches back to the context you had before.
set -euo pipefail
source "$(dirname "$0")/env.sh"
CTX="kind-$CLUSTER"
K() { kubectl --context "$CTX" "$@"; }

# The data the tools read (the databases, and a writable folder for curl)
# reaches the host pod through the kind node: a node path, mounted into the
# host container by k8s/values.yaml, and allowed by its hostPaths list. The
# private-data folder is on the host pod too, but no workload is granted it
# (step 3). Put the files in place before the cluster, because kind mounts
# these node paths at creation time.
"$ROOT/scripts/stage-data.sh"
if ! kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  # kind switches kubectl to the new cluster; make clean switches back.
  prev=$(kubectl config current-context 2>/dev/null || true)
  if [ -n "$prev" ] && [ "$prev" != "$CTX" ]; then
    echo "$prev" >"$RUN_DIR/previous-context"
  else
    rm -f "$RUN_DIR/previous-context"
  fi
  cat >"$RUN_DIR/kind-config.yaml" <<YAML
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
    extraPortMappings:
      - containerPort: 30180
        hostPort: ${HOST_HTTP##*:}
        listenAddress: "${HOST_HTTP%:*}"
    extraMounts:
      - hostPath: $RUN_DIR/sqlite-data
        containerPath: /workshop/sqlite-data
        readOnly: true
      - hostPath: $RUN_DIR/curl-data
        containerPath: /workshop/curl-data
      - hostPath: $RUN_DIR/private-data
        containerPath: /workshop/private-data
        readOnly: true
YAML
  kind create cluster --name "$CLUSTER" --config "$RUN_DIR/kind-config.yaml"
fi

# The upstream chart, from the v2.12.0 tag (gitignored, fetched once). The
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
  docker run -d --restart unless-stopped --name "$REGISTRY_CONTAINER" -p "127.0.0.1:$KIND_REGISTRY_PORT:5000" "$REGISTRY_IMAGE" >/dev/null
  sleep 1
fi
docker network connect kind "$REGISTRY_CONTAINER" 2>/dev/null || true
REGISTRY="localhost:$KIND_REGISTRY_PORT" "$ROOT/scripts/push.sh" "${SRC:-$ROOT/artifacts}"

# Load an image from Docker into the kind node, so the node never pulls it.
# Docker's containerd image store keeps only this machine's platform of a
# multi-platform image, which `kind load docker-image` (--all-platforms)
# rejects; save just that platform instead.
load_node_image() { # name
  if ! kind load docker-image "$1" --name "$CLUSTER" >/dev/null 2>&1; then
    archive="$RUN_DIR/node-image.tar"
    docker save --platform "$(docker version -f '{{.Server.Os}}/{{.Server.Arch}}')" -o "$archive" "$1"
    kind load image-archive "$archive" --name "$CLUSTER" >/dev/null
    rm -f "$archive"
  fi
}

# The workshop's host image (wash with the toolbox plugin compiled in,
# host/README.md) is loaded into the node, so the host pods start from it
# without pulling (k8s/values.yaml: pull_policy IfNotPresent). It is pulled
# by its pinned digest (scripts/env.sh), which Docker checks, and tagged with
# the name k8s/values.yaml uses.
if [ -n "$HOST_IMAGE_DIGEST" ]; then
  ref="${HOST_IMAGE%:*}@$HOST_IMAGE_DIGEST"
  docker image inspect "$ref" >/dev/null 2>&1 || docker pull -q "$ref" >/dev/null
  docker tag "$ref" "$HOST_IMAGE"
fi
load_node_image "$HOST_IMAGE"

# The mock-sites host group runs in mock-sites, which must exist first.
K apply -f "$ROOT/k8s/sites.yaml" >/dev/null
helm --kube-context "$CTX" upgrade --install wasmcloud "$ROOT/.upstream/charts/runtime-operator" \
  --namespace wasmcloud --create-namespace -f "$ROOT/k8s/values.yaml" --wait --timeout 10m
K -n wasmcloud rollout status deploy/hostgroup-default --timeout 5m >/dev/null
K -n mock-sites rollout status deploy/hostgroup-mock-sites --timeout 5m >/dev/null

# The ingress image is loaded into the node so nothing pulls it at the
# workshop. It is pulled by its pinned digest (scripts/env.sh), which Docker
# checks, then tagged with the name k8s/ingress.yaml uses and loaded into the
# node.
for ref in $NODE_IMAGES; do
  name="${ref%@*}"
  docker image inspect "$ref" >/dev/null 2>&1 || docker pull -q "$ref" >/dev/null
  docker tag "$ref" "$name"
  load_node_image "$name"
done
K apply -f "$ROOT/k8s/agent-service.yaml" >/dev/null
K apply -f "$ROOT/k8s/ingress.yaml" >/dev/null
K -n wasmcloud rollout status deploy/workshop-ingress --timeout 3m >/dev/null

# The mock sites (the api page and the attacker's server) run from the
# start, on the mock-sites host group. A host pulls an image when a workload
# starts, and the tag above is the same on every run, so recreate them to pick
# up what was just pushed.
K -n mock-sites delete workloaddeployment mock-sites --ignore-not-found --wait=true >/dev/null
K apply -f "$ROOT/mock-sites/workloaddeployment.yaml" >/dev/null
K -n mock-sites wait --for=condition=Ready workloaddeployment/mock-sites --timeout=180s >/dev/null || true
# The workshop guide (k8s/guide.yaml), on the same host group, recreated
# for the same reason.
K -n mock-sites delete workloaddeployment guide --ignore-not-found --wait=true >/dev/null
K apply -f "$ROOT/k8s/guide.yaml" >/dev/null
K -n mock-sites wait --for=condition=Ready workloaddeployment/guide --timeout=180s >/dev/null || true
for _ in $(seq 1 30); do
  curl -s -o /dev/null -m 2 "http://$HOST_HTTP/" && break; sleep 1
done

K get pods -n wasmcloud
K get pods -n mock-sites
echo
echo "cluster   kubectl context $CTX"
echo "registry  $REGISTRY_CONTAINER:5000 inside the cluster (127.0.0.1:$KIND_REGISTRY_PORT from here)"
echo "ingress   http://$HOST_HTTP  (use curl with a Host: header)"
echo "guide     http://guide.localhost:${HOST_HTTP##*:}  (the steps, in your browser)"
