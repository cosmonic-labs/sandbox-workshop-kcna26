# Shared settings. Every port is non-default so nothing collides with other
# local wasmCloud or NATS setups.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Tools fetched by `make tools` (scripts/tools.sh) win over anything on PATH.
export PATH="$ROOT/.tools:$PATH"
export WASH="${WASH:-$ROOT/.tools/wash}"          # pinned wash v2.11.0
export WASMCLOUD_VERSION=2.11.0                    # wash, host and chart
export CLUSTER=workshop-toolbox                    # kind cluster name
export RUN_DIR="$ROOT/.run"
export NATS_PORT=14222
export NATS_URL="nats://127.0.0.1:$NATS_PORT"
# The no-Docker fallback (make up-local) only: its registry, and how it runs.
# zot (default) is a single binary from .tools/; docker is a registry:2
# container. The cluster always uses $REGISTRY_CONTAINER, below.
export REGISTRY="${REGISTRY:-localhost:15051}"
export REGISTRY_MODE="${REGISTRY_MODE:-zot}"
export REGISTRY_CONTAINER="workshop-toolbox-registry"
export KIND_REGISTRY_PORT=15052
# The cluster's data service (k8s/data-nats.yaml), reachable from the laptop.
export DATA_NATS_URL="nats://127.0.0.1:14322"
export HOST_HTTP="${HOST_HTTP:-127.0.0.1:18180}"   # agent and sites ingress
export HOST_PROBE="${HOST_PROBE:-127.0.0.1:18181}"
export TAG="${TAG:-0.3.0}"
mkdir -p "$RUN_DIR"
