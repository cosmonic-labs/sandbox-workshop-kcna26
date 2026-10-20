#!/usr/bin/env bash
# The same proofs on kind + the upstream chart (scripts/kind-up.sh first).
# Writes k8s/expected-output.txt.
set -euo pipefail
source "$(dirname "$0")/env.sh"
cd "$ROOT"
export KUBECONFIG="$RUN_DIR/kubeconfig"
PF=127.0.0.1:18280
kubectl -n wasmcloud port-forward deploy/hostgroup-default "${PF##*:}:80" >"$RUN_DIR/pf.log" 2>&1 &
PFPID=$!; trap 'kill $PFPID 2>/dev/null' EXIT; sleep 3
ask() { echo "\$ curl -s -H 'Host: $1' http://$PF/ -d '$2'"; curl -s -m 30 -H "Host: $1" "http://$PF/" -d "$2"; echo; }
kapply() { echo "\$ scripts/kube-apply.sh $*"; ./scripts/kube-apply.sh "$@"; }
wait_ready() { kubectl wait --for=condition=Ready workloaddeployment --all --timeout=120s >/dev/null; sleep 3; }
{
  echo "# kind + charts/runtime-operator v2.10.3, same manifests (registry and notes path rewritten)"
  kapply steps/00-fixtures/toy-sites.yaml steps/06-web/workload.yaml; wait_ready
  echo "\$ kubectl get workloaddeployments,workloads"; kubectl get workloaddeployments,workloads; echo
  ask agent.localhost 'remember that my api key is sk-kind-42'
  ask agent.localhost 'read my notes'
  curl -s -X POST -H 'Host: sites.localhost' "http://$PF/reset" >/dev/null
  ask agent.localhost 'summarize http://planted.example/page'
  echo "\$ curl -s -H 'Host: sites.localhost' http://$PF/count"; curl -s -H 'Host: sites.localhost' "http://$PF/count"; echo
  echo "# Control: allowedHosts \"*\""
  kapply steps/06-web/control-wide-open.yaml; sleep 10; wait_ready
  curl -s -X POST -H 'Host: sites.localhost' "http://$PF/reset" >/dev/null
  ask agent.localhost 'summarize http://planted.example/page' | grep -E '^\$|\[tool (fetch|post)'
  echo "\$ curl -s -H 'Host: sites.localhost' http://$PF/count"; curl -s -H 'Host: sites.localhost' "http://$PF/count" | head -1; echo
  echo "# Scale subresource (what an HPA drives)"
  echo "\$ kubectl scale workloaddeployment toolbox-agent --replicas=3"
  kubectl scale workloaddeployment toolbox-agent --replicas=3; sleep 10; wait_ready
  kubectl get workloaddeployment toolbox-agent
  kapply steps/06-web/workload.yaml
} >k8s/expected-output.txt 2>&1
cat k8s/expected-output.txt
