# Shared settings. Every port is non-default so nothing collides with other
# local wasmCloud setups.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Tools fetched by `make tools` (scripts/tools.sh) win over anything on PATH.
export PATH="$ROOT/.tools:$PATH"
export WASH="${WASH:-$ROOT/.tools/wash}"          # pinned wash v2.11.0 (oci push)
export WASMCLOUD_VERSION=2.11.0                    # wash, host and chart
export CLUSTER=workshop-toolbox                    # kind cluster name
export RUN_DIR="$ROOT/.run"
# The registry container on the kind network, and its port on the laptop.
export REGISTRY_CONTAINER="workshop-toolbox-registry"
export KIND_REGISTRY_PORT=15052
export REGISTRY="${REGISTRY:-localhost:$KIND_REGISTRY_PORT}"
export HOST_HTTP="${HOST_HTTP:-127.0.0.1:18180}"   # agent and sites ingress
export HOST_PROBE="${HOST_PROBE:-127.0.0.1:18181}"
export TAG="${TAG:-0.4.0}"
mkdir -p "$RUN_DIR"
