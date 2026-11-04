#!/usr/bin/env bash
# Replay every step on the kind cluster (make up) and write each step's
# transcript to steps/<step>/expected-output.txt. Resets the demo state first
# so the outputs are repeatable. The commands echoed with "$" are the ones the
# step READMEs ask attendees to run. Extra checks that no README asks for (the
# wide-open control) go to steps/<step>/control-checks.txt.
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
apply() { # manifest: apply it and wait until every workload in it is ready
  echo "\$ kubectl apply -f $1 && $WAIT"
  kubectl apply -f "$1" >/dev/null
  sleep 3
  kubectl wait --for=condition=Ready workloaddeployment --all --all-namespaces --timeout=120s >/dev/null
  echo
}

# Clean slate: one host pod, no agent workloads, the toy sites running.
kubectl -n wasmcloud scale deployment hostgroup-default --replicas=1 >/dev/null
kubectl -n wasmcloud rollout status deployment/hostgroup-default --timeout=180s >/dev/null
kubectl delete workloaddeployment --all -n default >/dev/null 2>&1 || true
"$ROOT/scripts/stores.sh" reset >/dev/null
kubectl apply -f steps/00-fixtures/toy-sites.yaml >/dev/null
kubectl wait --for=condition=Ready workloaddeployment/toy-sites -n workshop-sites --timeout=120s >/dev/null

{
  echo "# Step 1: run the agent and its three tools"
  apply steps/01-run-it/workload.yaml
  echo "# sqlite reads the talks database:"
  ask agent.localhost 'list the talks in sqlite'
  echo "# git reads the repository:"
  ask agent.localhost 'show the recent commits'
  echo "# curl reaches the one host it is granted:"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
  echo "# The agent harness's imports from this workshop: the model and three typed, labelled tool slots"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'"
  wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
  echo "# The harness imports no HTTP client, no filesystem and no sockets, so this prints nothing:"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'"
  wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)' || true
  echo "# The tools are small. Look at the sizes:"
  echo "\$ ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm"
  ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm | awk '{print $5, $NF}'
} >"$(OUT 01-run-it)" 2>&1

{
  echo "# Step 2: the curl tool imports only an HTTP client; its reach is the grant"
  apply steps/02-build-curl/workload.yaml
  echo "\$ wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import|export'"
  wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import|export'
  echo "# It reaches the one host its manifest grants:"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
} >"$(OUT 02-build-curl)" 2>&1

{
  echo "# Step 3: curl reaches only the host it is granted (api.workshop-sites.svc)"
  apply steps/03-curl/workload.yaml
  echo "# Granted: the api host answers"
  ask agent.localhost 'curl -s http://api.workshop-sites.svc/data'
  echo "# The cloud metadata service: refused, it is not granted"
  ask agent.localhost 'curl -s http://169.254.169.254/latest/meta-data/'
  echo "# Another cluster host: refused"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
  echo "# A redirect from the granted host to a denied host: the second hop is refused"
  ask agent.localhost 'curl -sL http://api.workshop-sites.svc/redirect'
  echo "# Writing outside the tool's folder with -o: refused"
  ask agent.localhost 'curl -o /etc/evil -s http://api.workshop-sites.svc/data'
  echo "# Writing inside the tool's folder with -o: allowed"
  ask agent.localhost 'curl -o page.html -s http://api.workshop-sites.svc/data'
  echo "# Reading a file:// URL outside the tool's folder: refused"
  ask agent.localhost 'curl -s file:///etc/passwd'
  echo "# Reading a file:// URL inside the tool's folder: allowed"
  ask agent.localhost 'curl -s file:///data/page.html'
} >"$(OUT 03-curl)" 2>&1

{
  echo "# Step 3, make prove only: the grant is the only thing stopping it"
  echo "# Control: curl's allowedHosts widened to \"*\", nothing else changed."
  apply steps/03-curl/control-wide-open.yaml
  echo "# The same fetch that was refused now reaches the other host:"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
  apply steps/03-curl/workload.yaml >/dev/null
} >"$(CHECKS 03-curl)" 2>&1

{
  echo "# Step 4: sqlite over the database in its read-only folder"
  apply steps/04-sqlite/workload.yaml
  echo "# Ordinary SQL works:"
  ask agent.localhost "sqlite select title, speaker from talks where track = 'Security' order by title"
  echo "# Dot-commands work:"
  ask agent.localhost 'sqlite .tables'
  echo "# .shell cannot exist: there is no process to start"
  ask agent.localhost 'sqlite .shell id'
  echo "# ATTACH to a file outside the folder: refused"
  ask agent.localhost "sqlite attach '/etc/passwd' as evil"
  echo "# load_extension: not available"
  ask agent.localhost "sqlite select load_extension('/tmp/evil')"
} >"$(OUT 04-sqlite)" 2>&1

{
  echo "# Step 5: git reads a hostile repository, and none of its traps fire"
  apply steps/05-git/workload.yaml
  echo "# The repository's config wires core.fsmonitor, core.pager, aliases, a"
  echo "# smudge filter, a textconv and a credential helper to shell commands."
  echo "# Native git runs several of them just to read the repo. This git runs none."
  echo "# log reads commits:"
  ask agent.localhost 'git log --oneline -5'
  echo "# show reads a commit and its changed files:"
  ask agent.localhost 'git show HEAD'
  echo "# cat-file reads a tracked file (the smudge filter does not run):"
  ask agent.localhost 'git cat-file -p HEAD:README.md'
  echo "# blame reads line history:"
  ask agent.localhost 'git blame README.md'
  echo "# A command that writes or reaches the network: refused"
  ask agent.localhost 'git push origin main'
} >"$(OUT 05-git)" 2>&1

{
  echo "# Extra: a real model behind the same slot (a fake OpenAI compatible endpoint)"
  apply extras/real-model/workload.yaml
  echo "# The model asks for a tool, the agent runs it, the model answers:"
  ask agent.localhost 'what talks are there?'
} >extras/real-model/expected-output.txt 2>&1

echo "proofs written:"
ls steps/*/expected-output.txt steps/*/control-checks.txt
