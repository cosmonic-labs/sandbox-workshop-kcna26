#!/usr/bin/env bash
# The keyvalue stores on the cluster's data service (k8s/data-valkey.yaml).
# Valkey has no buckets: each store the platform declares in k8s/values.yaml
# is a key prefix, and a workload sees only the keys under its store's prefix.
#
#   scripts/stores.sh wait     wait until the data service answers (make up)
#   scripts/stores.sh keys     list every key, store prefix included
#   scripts/stores.sh reset    delete every store's keys (make prove)
set -euo pipefail
source "$(dirname "$0")/env.sh"
PREFIXES="agent: team-a: team-b: toy-sites:"
V() { kubectl --context "kind-$CLUSTER" -n workshop-data exec deploy/valkey -- valkey-cli "$@"; }
case "${1:-wait}" in
  wait)
    for _ in $(seq 1 60); do [ "$(V ping 2>/dev/null)" = PONG ] && break; sleep 2; done
    [ "$(V ping 2>/dev/null)" = PONG ] || { echo "stores    the data service does not answer" >&2; exit 1; }
    echo "stores    keyvalue on valkey.workshop-data.svc; stores (key prefixes): $PREFIXES"
    ;;
  keys)
    V --scan | sort
    ;;
  reset)
    for p in $PREFIXES; do
      V --scan --pattern "$p*" | while read -r k; do V del "$k" >/dev/null; done
    done
    echo "stores    emptied: $PREFIXES"
    ;;
  *) echo "usage: $0 wait|keys|reset" >&2; exit 2 ;;
esac
