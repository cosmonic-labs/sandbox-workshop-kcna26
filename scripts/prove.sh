#!/usr/bin/env bash
# Replay every step on the kind cluster (make up) and write each step's
# transcript to steps/<step>/expected-output.txt. Resets the demo state first
# so the outputs are repeatable. The commands echoed with "$" are the ones the
# step READMEs ask attendees to run. Extra checks that no README asks for
# (control cases, the squatter) go to steps/<step>/control-checks.txt.
set -euo pipefail
source "$(dirname "$0")/env.sh"
cd "$ROOT"
URL="http://$HOST_HTTP"
OUT() { echo "steps/$1/expected-output.txt"; }
CHECKS() { echo "steps/$1/control-checks.txt"; }
WAIT='sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent'
kubectl config use-context "kind-$CLUSTER" >/dev/null
ask() { # host, prompt
  echo "\$ curl -s -H 'Host: $1' $URL/ -d '$2'"
  curl -s -m 30 -H "Host: $1" "$URL/" -d "$2"
  echo
}
count() {
  echo "\$ curl -s -H 'Host: sites.localhost' $URL/count"
  curl -s -H 'Host: sites.localhost' "$URL/count"
  echo
}
reset_count() { curl -s -X POST -H 'Host: sites.localhost' "$URL/reset" >/dev/null; }
apply() { # manifest: apply it and wait until every workload in it is ready
  echo "\$ kubectl apply -f $1 && $WAIT"
  kubectl apply -f "$1"
  sleep 3
  kubectl wait --for=condition=Ready workloaddeployment --all --all-namespaces --timeout=120s >/dev/null
  echo
}
redeploy() { # manifest: stop and start it again
  echo "\$ kubectl delete -f $1 && kubectl apply -f $1"
  echo "\$ $WAIT"
  kubectl delete -f "$1" >/dev/null
  apply "$1" >/dev/null
  echo
}
hosts() {
  echo "\$ kubectl get workloads -A -o custom-columns=NAMESPACE:.metadata.namespace,HOST:.status.hostId"
  kubectl get workloads -A -o custom-columns=NAMESPACE:.metadata.namespace,HOST:.status.hostId
  echo "\$ kubectl get pods -A -l wasmcloud.com/name=hostgroup"
  kubectl get pods -A -l wasmcloud.com/name=hostgroup
  echo
}
STORES="$DATA_NATS_URL"
N() { nats --server "$STORES" "$@"; }

# Clean slate, with one host pod in the default host group (step 8 adds one).
kubectl -n wasmcloud scale deployment hostgroup-default --replicas=1 >/dev/null
kubectl -n wasmcloud rollout status deployment/hostgroup-default --timeout=180s >/dev/null
kubectl delete workloaddeployment --all -n default >/dev/null 2>&1 || true
kubectl delete -f steps/03-tenants/squatter.yaml --ignore-not-found >/dev/null 2>&1 || true
kubectl delete -f steps/03-tenants/tenants.yaml --ignore-not-found >/dev/null 2>&1 || true
kubectl delete -f steps/04-container/gullible-agent.yaml --ignore-not-found >/dev/null 2>&1 || true
"$ROOT/scripts/stores.sh" reset >/dev/null
kubectl apply -f steps/00-fixtures/toy-sites.yaml >/dev/null
kubectl wait --for=condition=Ready workloaddeployment/toy-sites -n workshop-sites --timeout=120s >/dev/null
reset_count

{
  echo "# Step 1: deploy the agent with kubectl; it runs on a wasmCloud host pod"
  apply steps/01-agent/workload.yaml
  ask agent.localhost 'hello'
  echo "# Every tool slot is filled by not-installed, so a tool call gets a clear error:"
  ask agent.localhost 'read my notes'
  hosts
  echo "# The agent harness's imports from this workshop: the model and three typed, labelled tool slots"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'"
  wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
  echo "# The agent harness imports no outgoing HTTP client, no filesystem and no sockets, so this prints nothing:"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'"
  wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)' || true
} >"$(OUT 01-agent)" 2>&1

{
  echo "# Step 2: the memory tool keeps facts across calls and across a redeploy"
  apply steps/02-memory/workload.yaml
  ask agent.localhost 'remember that my favorite color is teal'
  ask agent.localhost 'remember that my api key is sk-demo-123'
  redeploy steps/02-memory/workload.yaml
  ask agent.localhost 'what do you remember?'
  echo "\$ nats --server $STORES kv ls agent-memory"
  N kv ls agent-memory
} >"$(OUT 02-memory)" 2>&1

{
  echo "# Step 3: two teams, two namespaces, one host pod"
  echo "\$ kubectl apply -f steps/03-tenants/tenants.yaml && sleep 2"
  echo "\$ kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent -n team-a"
  echo "\$ kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent -n team-b"
  apply steps/03-tenants/tenants.yaml | sed 1d
  ask a.agent.localhost 'remember that our team is alpha'
  ask b.agent.localhost 'remember that our team is bravo'
  ask a.agent.localhost 'what do you remember?'
  ask b.agent.localhost 'what do you remember?'
  echo "# Both teams' workloads, and step 2's, run on the same host; the toy sites run on their own:"
  hosts
} >"$(OUT 03-tenants)" 2>&1

{
  echo "# Step 3, make prove only: the squatter check"
  echo "# Squatter check: team-b claims the toy sites' names on the host it shares with team-a."
  echo "# team-a's agent gets step 5's web tool (allowed planted.workshop-sites.svc only)."
  sed -e '/^  name: toolbox-agent$/a\
  namespace: team-a' -e 's/host: agent.localhost/host: a.agent.localhost/' \
    -e 's/MEMORY_BUCKET: agent-memory/MEMORY_BUCKET: tenant-a-memory/' \
    steps/05-web/workload.yaml >"$RUN_DIR/team-a-web.yaml"
  apply .run/team-a-web.yaml >/dev/null
  apply steps/03-tenants/squatter.yaml
  echo "# Six fetches from team-a; with local routing on, the squatter would answer them."
  real=0
  for _ in 1 2 3 4 5 6; do
    curl -s -m 30 -H 'Host: a.agent.localhost' "$URL/" -d 'fetch http://planted.workshop-sites.svc/page' \
      | grep -q 'Totally normal release notes' && real=$((real + 1))
  done
  echo "\$ curl -s -H 'Host: a.agent.localhost' $URL/ -d 'fetch http://planted.workshop-sites.svc/page'   # x6"
  echo "answered by the toy sites: $real of 6; by the squatter: $((6 - real)) of 6"
  kubectl delete -f steps/03-tenants/squatter.yaml >/dev/null
  apply steps/03-tenants/tenants.yaml >/dev/null
} >"$(CHECKS 03-tenants)" 2>&1

{
  echo "# Step 4: the same gullible agent as an ordinary container"
  reset_count
  echo "\$ kubectl apply -f steps/04-container/gullible-agent.yaml"
  kubectl apply -f steps/04-container/gullible-agent.yaml
  kubectl wait --for=condition=complete job/gullible-agent --timeout=120s >/dev/null
  echo
  echo "\$ kubectl logs job/gullible-agent"
  kubectl logs job/gullible-agent
  echo
  # The collector got the whole service account token; keep it out of the
  # committed transcript, which would change on every run.
  count | sed -E '/^last=/s/(eyJ[A-Za-z0-9_-]{17})[A-Za-z0-9_.-]+/\1...(the full token)/'
  echo "# Play the attacker: take the token from the collector and use it (ids masked)"
  echo "\$ TOKEN=\$(curl -s -H 'Host: sites.localhost' $URL/leaks \\"
  echo "  | grep -o 'service account token: [^\"]*' | head -1 | cut -d' ' -f4)"
  echo "\$ API=\$(kubectl config view --minify -o jsonpath='{.clusters[0].cluster.server}')"
  TOKEN=$(curl -s -H 'Host: sites.localhost' "$URL/leaks" | grep -o 'service account token: [^"]*' | head -1 | cut -d' ' -f4)
  API=$(kubectl config view --minify -o jsonpath='{.clusters[0].cluster.server}')
  KT() { kubectl --kubeconfig=/dev/null -s "$API" --insecure-skip-tls-verify --token "$TOKEN" "$@" 2>&1 || true; }
  echo "\$ kubectl --kubeconfig=/dev/null -s \"\$API\" --insecure-skip-tls-verify --token \"\$TOKEN\" auth whoami"
  KT auth whoami | sed -E -e 's/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/(an id)/g' \
    -e 's/gullible-agent-[a-z0-9]+/gullible-agent-(suffix)/'
  echo
  echo "\$ kubectl --kubeconfig=/dev/null -s \"\$API\" --insecure-skip-tls-verify --token \"\$TOKEN\" get secrets"
  KT get secrets
} >"$(OUT 04-container)" 2>&1

{
  echo "# Step 5: planted attack 1 (the web tool may reach planted.workshop-sites.svc only)"
  apply steps/05-web/workload.yaml
  reset_count
  ask agent.localhost 'summarize http://planted.workshop-sites.svc/page'
  count
  echo "# The allowed host is a way out too: page2 says to send everything to planted.workshop-sites.svc"
  ask agent.localhost 'summarize http://planted.workshop-sites.svc/page2'
  count
  echo "# Control: the web tool's allowedHosts widened to \"*\", nothing else changed"
  apply steps/05-web/control-wide-open.yaml
  echo "\$ curl -s -X POST -H 'Host: sites.localhost' $URL/reset"
  reset_count
  ask agent.localhost 'summarize http://planted.workshop-sites.svc/page'
  count
  apply steps/05-web/workload.yaml >/dev/null
} >"$(OUT 05-web)" 2>&1

{
  echo "# Step 5, make prove only: grants are per component"
  echo "# Per component, not per workload: the model may reach llm.workshop-sites.svc, the web tool may not"
  sed -e '/name: model$/{n;s#$#\n          localResources:\n            allowedHosts: [llm.workshop-sites.svc]#;}' \
    steps/05-web/workload.yaml >"$RUN_DIR/per-component.yaml"
  apply .run/per-component.yaml
  ask agent.localhost 'fetch http://llm.workshop-sites.svc/v1/chat/completions'
  apply steps/05-web/workload.yaml >/dev/null
} >"$(CHECKS 05-web)" 2>&1

{
  echo "# Step 6: planted attack 2 (a read only folder, mounted for the notes tool only)"
  apply steps/06-notes/workload.yaml
  ask agent.localhost 'read my notes'
  echo "\$ tail -1 steps/06-notes/notes/todo.txt   # unchanged"
  tail -1 steps/06-notes/notes/todo.txt
} >"$(OUT 06-notes)" 2>&1

{
  echo "# Step 6, make prove only: the same manifest, read-write"
  echo "# Control: the same manifest with readOnly: false, on a scratch copy of the notes"
  rm -rf "$RUN_DIR/notes-rw"/* && cp -R steps/06-notes/notes/. "$RUN_DIR/notes-rw/"
  sed -e 's/readOnly: true/readOnly: false/' -e 's#path: /workshop/notes$#path: /workshop/notes-rw#' \
    steps/06-notes/workload.yaml >"$RUN_DIR/rw.yaml"
  apply .run/rw.yaml
  ask agent.localhost 'read my notes'
  echo "\$ tail -1 .run/notes-rw/todo.txt"
  tail -1 "$RUN_DIR/notes-rw/todo.txt"
  apply steps/06-notes/workload.yaml >/dev/null
} >"$(CHECKS 06-notes)" 2>&1

{
  echo "# Step 7: model-real behind the same slot (a fake OpenAI compatible endpoint on llm.workshop-sites.svc)"
  apply steps/07-real-model/workload.yaml
  ask agent.localhost 'hello'
  ask agent.localhost 'recall what you know about me'
} >"$(OUT 07-real-model)" 2>&1

{
  echo "# Step 8: scale across hosts, then troubleshoot (host placement is random; ids vary)"
  echo "\$ kubectl -n wasmcloud scale deployment hostgroup-default --replicas=2"
  kubectl -n wasmcloud scale deployment hostgroup-default --replicas=2
  kubectl -n wasmcloud rollout status deployment/hostgroup-default --timeout=180s >/dev/null
  sleep 3
  echo "\$ kubectl get hosts -n wasmcloud"
  kubectl get hosts -n wasmcloud
  echo "\$ kubectl scale workloaddeployment toolbox-agent --replicas=3"
  kubectl scale workloaddeployment toolbox-agent --replicas=3
  sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent --timeout=120s >/dev/null
  echo "\$ kubectl get workloads -n default -o custom-columns=NAME:.metadata.name,HOST:.status.hostId"
  kubectl get workloads -n default -o custom-columns=NAME:.metadata.name,HOST:.status.hostId
  echo "\$ kubectl get endpointslices -l kubernetes.io/service-name=toolbox-agent"
  kubectl get endpointslices -l kubernetes.io/service-name=toolbox-agent
  echo "\$ for i in 1 2 3 4 5 6; do curl -s -o /dev/null -w '%{http_code} ' -H 'Host: agent.localhost' $URL/ -d 'hello'; done; echo"
  for i in 1 2 3 4 5 6; do curl -s -o /dev/null -w '%{http_code} ' -H 'Host: agent.localhost' "$URL/" -d 'hello'; done; echo
  ask agent.localhost 'recall what you know about me'
  echo "\$ kubectl -n wasmcloud scale deployment hostgroup-default --replicas=1"
  kubectl -n wasmcloud scale deployment hostgroup-default --replicas=1
  kubectl -n wasmcloud rollout status deployment/hostgroup-default --timeout=180s >/dev/null
  sleep 5
  echo "\$ kubectl get workloads -n default -o custom-columns=NAME:.metadata.name,HOST:.status.hostId"
  kubectl get workloads -n default -o custom-columns=NAME:.metadata.name,HOST:.status.hostId
  echo "\$ kubectl scale workloaddeployment toolbox-agent --replicas=1"
  kubectl scale workloaddeployment toolbox-agent --replicas=1
  echo
  echo "# Troubleshoot 1: a missing host interface"
  echo "\$ kubectl apply -f steps/08-scale/broken-interface.yaml"
  kubectl apply -f steps/08-scale/broken-interface.yaml
  sleep 8
  echo "\$ kubectl describe workloads -n default | grep -E '^Name:|Message:'"
  kubectl describe workloads -n default | grep -E '^Name:|Message:' || true
  echo "# The previous version still answers:"
  ask agent.localhost 'hello'
  apply steps/07-real-model/workload.yaml >/dev/null
  echo "# Troubleshoot 2: a typo in an image name"
  echo "\$ kubectl apply -f steps/08-scale/broken-image.yaml"
  kubectl apply -f steps/08-scale/broken-image.yaml
  sleep 8
  echo "\$ kubectl describe workloads -n default | grep -E '^Name:|Message:'"
  kubectl describe workloads -n default | grep -E '^Name:|Message:' || true
  echo "\$ kubectl logs -n wasmcloud deployment/hostgroup-default --since=5m | grep 'failed to start'"
  kubectl logs -n wasmcloud deployment/hostgroup-default --since=5m | grep 'failed to start' | tail -1 || true
  apply steps/07-real-model/workload.yaml >/dev/null
  echo
  echo "# Troubleshoot 3: a typo in the bucket's name"
  apply steps/08-scale/broken-bucket.yaml
  ask agent.localhost 'recall what you know about me'
  apply steps/07-real-model/workload.yaml >/dev/null
} >"$(OUT 08-scale)" 2>&1

echo "proofs written:"
ls steps/*/expected-output.txt steps/*/control-checks.txt
