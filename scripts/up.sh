#!/usr/bin/env bash
# Start a local upstream wasmCloud v2 stack with no Kubernetes:
#   nats-server (JetStream) -> scheduler bus + keyvalue/blobstore backend
#   registry:2 container    -> OCI registry the host pulls components from
#   wash host               -> the wasmCloud host (same binary the chart runs)
set -euo pipefail
source "$(dirname "$0")/env.sh"

if ! nc -z 127.0.0.1 "$NATS_PORT" 2>/dev/null; then
  nats-server -js -a 127.0.0.1 -p "$NATS_PORT" -sd "$RUN_DIR/jetstream" \
    >"$RUN_DIR/nats.log" 2>&1 &
  echo $! >"$RUN_DIR/nats.pid"
fi

if ! docker ps --format '{{.Names}}' | grep -qx "$REGISTRY_CONTAINER"; then
  docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1 || true
  docker run -d --name "$REGISTRY_CONTAINER" -p "127.0.0.1:${REGISTRY##*:}:5000" registry:2 >/dev/null
fi

sleep 1
host_running() { [ -f "$RUN_DIR/host.pid" ] && kill -0 "$(cat "$RUN_DIR/host.pid")" 2>/dev/null; }
if ! host_running; then
  if nc -z 127.0.0.1 "${HOST_HTTP##*:}" 2>/dev/null; then
    echo "port ${HOST_HTTP##*:} is already in use by something else; set HOST_HTTP" >&2
    exit 1
  fi
  rm -f "$RUN_DIR/host.id"
  "$WASH" host \
    --host-name workshop-host \
    --scheduler-nats-url "$NATS_URL" \
    --data-nats-url "$NATS_URL" \
    --http-addr "$HOST_HTTP" \
    --probe-addr "$HOST_PROBE" \
    --http-local-routing \
    --allow-insecure-registries \
    >"$RUN_DIR/host.log" 2>&1 &
  echo $! >"$RUN_DIR/host.pid"
fi

# The host announces itself on runtime.operator.heartbeat.<host-id>.
for _ in $(seq 1 60); do
  hb=$(grep -o 'host_id=[^ ]*' "$RUN_DIR/host.log" 2>/dev/null | head -1 | cut -d= -f2 | tr -d '"' || true)
  if [ -n "$hb" ]; then echo "$hb" >"$RUN_DIR/host.id"; break; fi
  sleep 0.5
done
echo "nats      $NATS_URL"
echo "registry  $REGISTRY"
echo "host      http://$HOST_HTTP  (id: $(cat "$RUN_DIR/host.id" 2>/dev/null || echo unknown))"
