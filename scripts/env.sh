# Shared settings. Every port is non-default so nothing collides with other
# local wasmCloud setups.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Tools fetched by `make tools` (scripts/tools.sh) win over anything on PATH.
export PATH="$ROOT/.tools:$PATH"
export WASH="${WASH:-$ROOT/.tools/wash}"          # pinned wash v2.12.0 (oci push)
export WASMCLOUD_VERSION=2.12.0                    # wash, chart, and the host the
                                                   # workshop's host image builds on
export CLUSTER=workshop-toolbox                    # kind cluster name
export RUN_DIR="$ROOT/.run"
# The registry container on the kind network, and its port on the laptop.
export REGISTRY_CONTAINER="workshop-toolbox-registry"
export KIND_REGISTRY_PORT=15052
export REGISTRY="${REGISTRY:-localhost:$KIND_REGISTRY_PORT}"
export HOST_HTTP="${HOST_HTTP:-127.0.0.1:28180}"   # agent and guide ingress
export TAG="${TAG:-0.4.0}"
# The workshop's host: wash v2.12.0 with the toolbox host plugin compiled in
# (host/, scripts/build-host.sh). k8s/values.yaml names the same image.
# up.sh pulls the published image by this digest (the multi-platform index),
# which Docker checks, and loads it into the node under HOST_IMAGE. Set
# HOST_IMAGE_DIGEST= (empty) to load your own build of HOST_IMAGE instead.
export HOST_IMAGE="${HOST_IMAGE:-ghcr.io/cosmonic-labs/workshop-host:2.12.0-toolbox.1}"
export HOST_IMAGE_DIGEST="${HOST_IMAGE_DIGEST-sha256:e9a4bc5de4acadb73b2e626848a0bd1ca4d12105357677e7d8d13b25548a6463}"
# Container images, pinned by digest (the multi-platform index digest).
export REGISTRY_IMAGE="docker.io/library/registry:2@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373"
# up.sh pulls this by digest, tags it, and loads it into the kind node, so
# the node never pulls it by tag (k8s/ingress.yaml).
export NODE_IMAGES="docker.io/library/nginx:1.27.5-alpine@sha256:65645c7bb6a0661892a8b03b89d0743208a18dd2f3f17a54ef4b76fb8e2f2a10"
mkdir -p "$RUN_DIR"
