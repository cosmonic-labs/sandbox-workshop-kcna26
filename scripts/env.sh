# Shared settings. Every port is non-default so nothing collides with other
# local wasmCloud or NATS setups.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Tools fetched by `make tools` (scripts/tools.sh) win over anything on PATH.
export PATH="$ROOT/.tools:$PATH"
export WASH="${WASH:-$ROOT/.tools/wash}"          # pinned wash v2.10.3
export RUN_DIR="$ROOT/.run"
export NATS_PORT=14222
export NATS_URL="nats://127.0.0.1:$NATS_PORT"
export REGISTRY="${REGISTRY:-localhost:15051}"
# zot (default): a single binary OCI registry from .tools/, no Docker needed.
# docker: the registry:2 container, as before. The kind track always uses the
# container, because the cluster pulls from it over the kind network.
export REGISTRY_MODE="${REGISTRY_MODE:-zot}"
export REGISTRY_CONTAINER="workshop-toolbox-registry"
export HOST_HTTP="${HOST_HTTP:-127.0.0.1:18180}"   # agent and sites ingress
export HOST_PROBE="${HOST_PROBE:-127.0.0.1:18181}"
export TAG="${TAG:-0.1.0}"
mkdir -p "$RUN_DIR"
