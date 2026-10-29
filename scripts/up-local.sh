#!/usr/bin/env bash
# The no-Docker fallback: a local upstream wasmCloud v2 stack with no Kubernetes
# (the workshop itself runs on kind, scripts/up.sh). Apply steps with
# scripts/apply.py instead of kubectl.
#   nats-server (JetStream) -> scheduler bus + keyvalue backend
#   zot (or registry:2)     -> OCI registry the host pulls components from
#   wash host               -> the wasmCloud host (same binary the chart runs)
set -euo pipefail
source "$(dirname "$0")/env.sh"
mkdir -p "$RUN_DIR/notes-rw"

if ! nc -z 127.0.0.1 "$NATS_PORT" 2>/dev/null; then
  nats-server -js -a 127.0.0.1 -p "$NATS_PORT" -sd "$RUN_DIR/jetstream" \
    >"$RUN_DIR/nats.log" 2>&1 &
  echo $! >"$RUN_DIR/nats.pid"
fi

REG_PORT="${REGISTRY##*:}"
if [ "$REGISTRY_MODE" = docker ]; then
  if ! docker ps --format '{{.Names}}' | grep -qx "$REGISTRY_CONTAINER"; then
    docker rm -f "$REGISTRY_CONTAINER" >/dev/null 2>&1 || true
    docker run -d --name "$REGISTRY_CONTAINER" -p "127.0.0.1:$REG_PORT:5000" registry:2 >/dev/null
  fi
elif ! nc -z 127.0.0.1 "$REG_PORT" 2>/dev/null; then
  if [ ! -x "$ROOT/.tools/zot" ]; then
    echo "no registry: .tools/zot is missing. Run 'make tools' (or set REGISTRY_MODE=docker)." >&2
    exit 1
  fi
  # Blobs and manifests live on disk in .run/registry, so a restart keeps
  # what you pushed. No TLS and no auth: it listens on 127.0.0.1 only.
  mkdir -p "$RUN_DIR/registry"
  cat >"$RUN_DIR/zot.json" <<JSON
{"distSpecVersion": "1.1.1",
 "storage": {"rootDirectory": "$RUN_DIR/registry", "gc": false, "dedupe": false},
 "http": {"address": "127.0.0.1", "port": "$REG_PORT"},
 "log": {"level": "warn"}}
JSON
  "$ROOT/.tools/zot" serve "$RUN_DIR/zot.json" >"$RUN_DIR/registry.log" 2>&1 &
  echo $! >"$RUN_DIR/registry.pid"
  for _ in $(seq 1 20); do nc -z 127.0.0.1 "$REG_PORT" 2>/dev/null && break; sleep 0.25; done
fi

sleep 1
host_running() { [ -f "$RUN_DIR/host.pid" ] && kill -0 "$(cat "$RUN_DIR/host.pid")" 2>/dev/null; }
if ! host_running; then
  if nc -z 127.0.0.1 "${HOST_HTTP##*:}" 2>/dev/null; then
    echo "port ${HOST_HTTP##*:} is already in use by something else; set HOST_HTTP" >&2
    exit 1
  fi
  rm -f "$RUN_DIR/host.id"
  # The same platform declaration as the chart's plugins: (k8s/values.yaml):
  # keyvalue is backed by this NATS, and workloads only ask.
  # A plugin reaches this machine's loopback only with --allow-host-loopback
  # and a port named in its own allowedHostLoopbackPorts.
  cat >"$RUN_DIR/host-config.yaml" <<YAML
host:
  plugins:
    - id: wasmcloud-keyvalue-multiplexed
      config: { backend: nats, url: "$NATS_URL" }
      allowedHosts: ["127.0.0.1:$NATS_PORT"]
      allowedHostLoopbackPorts: ["$NATS_PORT"]
YAML
  "$WASH" host \
    --user-config "$RUN_DIR/host-config.yaml" \
    --allow-host-loopback \
    --host-name workshop-host \
    --scheduler-nats-url "$NATS_URL" \
    --data-nats-url "$NATS_URL" \
    --http-addr "$HOST_HTTP" \
    --probe-addr "$HOST_PROBE" \
    --http-local-routing \
    --allow-insecure-registries \
    --allowed-host-path "$ROOT/steps/06-notes/notes" \
    --allowed-host-path "$RUN_DIR/notes-rw" \
    >"$RUN_DIR/host.log" 2>&1 &
  echo $! >"$RUN_DIR/host.pid"
fi

# The host announces itself on runtime.operator.heartbeat.<host-id>.
for _ in $(seq 1 60); do
  hb=$(grep -o 'host_id=[^ ]*' "$RUN_DIR/host.log" 2>/dev/null | head -1 | cut -d= -f2 | tr -d '"' || true)
  if [ -n "$hb" ]; then echo "$hb" >"$RUN_DIR/host.id"; break; fi
  sleep 0.5
done
STORES_URL="$NATS_URL" "$ROOT/scripts/stores.sh" create
echo "nats      $NATS_URL"
echo "registry  $REGISTRY ($REGISTRY_MODE)"
echo "host      http://$HOST_HTTP  (id: $(cat "$RUN_DIR/host.id" 2>/dev/null || echo unknown))"
