#!/usr/bin/env bash
# Replay every step on the kind cluster (make up) and write each step's
# transcript to steps/<step>/expected-output.txt. With LOCAL=1 (make
# prove-local) it replays them on the no-Docker fallback instead, applying
# with scripts/apply.py, skipping step 4 (it needs Kubernetes), and writing
# the transcripts to .run/local/. Resets the demo state first
# so the outputs are repeatable. The commands echoed with "$" are the ones the
# step READMEs ask attendees to run.
set -euo pipefail
source "$(dirname "$0")/env.sh"
cd "$ROOT"
LOCAL="${LOCAL:-0}"
URL="http://$HOST_HTTP"
OUT() { if [ "$LOCAL" = 1 ]; then mkdir -p "$RUN_DIR/local"; echo "$RUN_DIR/local/$1.txt"; else echo "steps/$1/expected-output.txt"; fi; }
if [ "$LOCAL" = 0 ]; then kubectl config use-context "kind-$CLUSTER" >/dev/null; fi
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
  if [ "$LOCAL" = 1 ]; then
    echo "\$ scripts/apply.py $1"; python3 scripts/apply.py "$1"; echo; return
  fi
  echo "\$ kubectl apply -f $1"
  kubectl apply -f "$1"
  sleep 3
  kubectl wait --for=condition=Ready workloaddeployment --all --all-namespaces --timeout=120s >/dev/null
  echo
}
redeploy() { # manifest: stop and start it again
  if [ "$LOCAL" = 1 ]; then
    echo "\$ scripts/apply.py --delete $1 && scripts/apply.py $1"
    python3 scripts/apply.py --delete "$1" >/dev/null; python3 scripts/apply.py "$1" >/dev/null; echo; return
  fi
  echo "\$ kubectl delete -f $1 && kubectl apply -f $1"
  kubectl delete -f "$1" >/dev/null
  apply "$1" >/dev/null
  echo
}
hosts() {
  [ "$LOCAL" = 1 ] && { echo "# (one wash host; nothing to list)"; return; }
  echo "\$ kubectl get workloads -A -o custom-columns=NAMESPACE:.metadata.namespace,HOST:.status.hostId"
  kubectl get workloads -A -o custom-columns=NAMESPACE:.metadata.namespace,HOST:.status.hostId
  echo "\$ kubectl get pods -n wasmcloud -l wasmcloud.com/name=hostgroup"
  kubectl get pods -n wasmcloud -l wasmcloud.com/name=hostgroup -o custom-columns=NAME:.metadata.name,READY:.status.phase
  echo
}
if [ "$LOCAL" = 1 ]; then STORES="$NATS_URL"; else STORES="$DATA_NATS_URL"; fi
N() { nats --server "$STORES" "$@"; }

# Clean slate.
if [ "$LOCAL" = 1 ]; then
  python3 scripts/apply.py --delete steps/03-tenants/tenants.yaml steps/07-web/workload.yaml >/dev/null 2>&1 || true
  STORES_URL="$NATS_URL" "$ROOT/scripts/stores.sh" reset >/dev/null
  python3 scripts/apply.py steps/00-fixtures/toy-sites.yaml >/dev/null
else
  kubectl delete workloaddeployment --all -n default >/dev/null 2>&1 || true
  kubectl delete -f steps/03-tenants/tenants.yaml --ignore-not-found >/dev/null 2>&1 || true
  kubectl delete -f steps/04-container/gullible-agent.yaml --ignore-not-found >/dev/null 2>&1 || true
  "$ROOT/scripts/stores.sh" reset >/dev/null
  kubectl apply -f steps/00-fixtures/toy-sites.yaml >/dev/null
  kubectl wait --for=condition=Ready workloaddeployment/toy-sites --timeout=120s >/dev/null
fi
reset_count

{
  echo "# Step 1: deploy the agent with kubectl; it runs on a wasmCloud host pod"
  apply steps/01-agent/workload.yaml
  ask agent.localhost 'hello'
  echo "# Every tool slot is filled by not-installed, so a tool call gets a clear error:"
  ask agent.localhost 'read my notes'
  hosts
  echo "# The agent harness and model import no filesystem, no outgoing HTTP, no sockets:"
  for c in agent model_scripted; do
    echo "\$ wasm-tools component wit artifacts/$c.wasm | grep -E 'filesystem|http/client|sockets' || echo none"
    wasm-tools component wit "artifacts/$c.wasm" | grep -E 'filesystem|http/client|sockets' || echo "$c: none"
  done
  echo "# The agent harness's imports from this workshop: the model and three typed, labelled tool slots"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'"
  wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
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
  apply steps/03-tenants/tenants.yaml
  ask a.agent.localhost 'remember that our team is alpha'
  ask b.agent.localhost 'remember that our team is bravo'
  ask a.agent.localhost 'what do you remember?'
  ask b.agent.localhost 'what do you remember?'
  echo "# Both teams' workloads, and step 2's, run on the same host:"
  hosts
} >"$(OUT 03-tenants)" 2>&1

[ "$LOCAL" = 1 ] || {
  echo "# Step 4: the same gullible agent as an ordinary container"
  reset_count
  echo "\$ kubectl apply -f steps/04-container/gullible-agent.yaml"
  kubectl apply -f steps/04-container/gullible-agent.yaml
  kubectl wait --for=condition=complete job/gullible-agent --timeout=120s >/dev/null
  echo
  echo "\$ kubectl logs job/gullible-agent"
  kubectl logs job/gullible-agent
  echo
  count
} >"$(OUT 04-container)" 2>&1

{
  echo "# Step 5: planted attack 1 (a read only folder, mounted for the notes tool only)"
  apply steps/05-notes/workload.yaml
  ask agent.localhost 'read my notes'
  echo "\$ tail -1 steps/05-notes/notes/todo.txt   # unchanged"
  tail -1 steps/05-notes/notes/todo.txt
  echo
  echo "# Control: the same manifest with readOnly: false, on a scratch copy of the notes"
  rm -rf "$RUN_DIR/notes-rw"/* && cp -R steps/05-notes/notes/. "$RUN_DIR/notes-rw/"
  sed -e 's/readOnly: true/readOnly: false/' -e 's#path: /workshop/notes$#path: /workshop/notes-rw#' \
    steps/05-notes/workload.yaml >"$RUN_DIR/rw.yaml"
  apply .run/rw.yaml
  ask agent.localhost 'read my notes'
  echo "\$ tail -1 .run/notes-rw/todo.txt"
  tail -1 "$RUN_DIR/notes-rw/todo.txt"
  apply steps/05-notes/workload.yaml >/dev/null
} >"$(OUT 05-notes)" 2>&1

{
  echo "# Step 7: planted attack 2 (the web tool may reach planted.example only)"
  apply steps/07-web/workload.yaml
  reset_count
  ask agent.localhost 'summarize http://planted.example/page'
  count
  echo "# Control: the web tool's allowedHosts widened to \"*\", nothing else changed"
  apply steps/07-web/control-wide-open.yaml
  reset_count
  ask agent.localhost 'summarize http://planted.example/page'
  count
  echo "# Per component, not per workload: the model may reach llm.example, the web tool may not"
  sed -e '/name: model$/{n;s#$#\n          localResources:\n            allowedHosts: [llm.example]#;}' \
    steps/07-web/workload.yaml >"$RUN_DIR/per-component.yaml"
  apply .run/per-component.yaml
  ask agent.localhost 'fetch http://llm.example/v1/chat/completions'
  apply steps/07-web/workload.yaml >/dev/null
} >"$(OUT 07-web)" 2>&1

{
  echo "# Step 8: model-real behind the same slot (a fake OpenAI compatible endpoint on llm.example)"
  apply steps/08-real-model/workload.yaml
  ask agent.localhost 'hello'
  ask agent.localhost 'recall what you know about me'
} >"$(OUT 08-real-model)" 2>&1

echo "proofs written:"
if [ "$LOCAL" = 1 ]; then ls "$RUN_DIR"/local/*.txt; else ls steps/*/expected-output.txt; fi
