# Shared settings. Every port is non-default so nothing collides with other
# local wasmCloud or NATS setups.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Tools fetched by `make tools` (scripts/tools.sh) win over anything on PATH.
export PATH="$ROOT/.tools:$PATH"
export WASH="${WASH:-$ROOT/.tools/wash}"          # pinned wash v2.12.0 (oci push)
export WASMCLOUD_VERSION=2.12.0                    # wash, chart, and the host image
                                                   # (2.12.0-all-features, k8s/values.yaml)
export CLUSTER=workshop-toolbox                    # kind cluster name
export RUN_DIR="$ROOT/.run"
# The registry container on the kind network, and its port on the laptop.
export REGISTRY_CONTAINER="workshop-toolbox-registry"
export KIND_REGISTRY_PORT=15052
export REGISTRY="${REGISTRY:-localhost:$KIND_REGISTRY_PORT}"
# The cluster's data service (k8s/data-nats.yaml), reachable from the laptop.
export DATA_NATS_URL="nats://127.0.0.1:14322"
export HOST_HTTP="${HOST_HTTP:-127.0.0.1:18180}"   # agent and sites ingress
export HOST_PROBE="${HOST_PROBE:-127.0.0.1:18181}"
export TAG="${TAG:-0.4.0}"
# Container images, pinned by digest (the multi-platform index digest).
export REGISTRY_IMAGE="docker.io/library/registry:2@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373"
# up.sh pulls this by digest, tags it, and loads it into the kind node, so
# the node never pulls it by tag (k8s/ingress.yaml).
export NODE_IMAGES="docker.io/library/nginx:1.27.5-alpine@sha256:65645c7bb6a0661892a8b03b89d0743208a18dd2f3f17a54ef4b76fb8e2f2a10"
mkdir -p "$RUN_DIR"
