#!/usr/bin/env bash
# The key value buckets the workshop's workloads use, on the cluster's data
# service (or, with NATS_URL, on the local fallback's NATS).
# The platform creates them; a workload may open a store, never create one.
#
#   scripts/stores.sh create   create any that are missing
#   scripts/stores.sh reset    delete and recreate them all (make prove)
set -euo pipefail
source "$(dirname "$0")/env.sh"
SERVER="${STORES_URL:-$DATA_NATS_URL}"
KV="agent-memory toy-collector tenant-a-memory tenant-b-memory"
N() { nats --server "$SERVER" "$@"; }
for _ in $(seq 1 30); do N server check connection >/dev/null 2>&1 && break; sleep 1; done
if [ "${1:-create}" = reset ]; then
  for b in $KV; do N kv del "$b" -f >/dev/null 2>&1 || true; done
fi
for b in $KV; do N kv info "$b" >/dev/null 2>&1 || N kv add "$b" >/dev/null; done
echo "stores    keyvalue: $KV"
