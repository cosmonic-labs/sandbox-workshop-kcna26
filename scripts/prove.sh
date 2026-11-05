#!/usr/bin/env bash
# Replay every step on the kind cluster (make up) and write each step's
# transcript to steps/<step>/expected-output.txt. Resets the demo state first
# so the outputs are repeatable. The commands echoed with "$" are the ones the
# step READMEs ask attendees to run, quoted so they paste into a shell. Extra
# checks that no README asks for (the controls) go to
# steps/<step>/control-checks.txt.
#
# Pod names in the demo namespace are random, so they are written as
# hello-<pod>. Everything else is deterministic: a second run on a fresh
# cluster should leave `git diff steps` empty.
set -euo pipefail
source "$(dirname "$0")/env.sh"
cd "$ROOT"
URL="http://$HOST_HTTP"
OUT() { echo "steps/$1/expected-output.txt"; }
CHECKS() { echo "steps/$1/control-checks.txt"; }
# What the READMEs tell attendees to type. prove itself waits more precisely
# (rollout below), so a slow machine can't record a reply from the old version.
WAIT='sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent'
kubectl config use-context "kind-$CLUSTER" >/dev/null

# A prompt as a shell word: single quotes, unless it holds one, then double
# quotes with \ " $ and ` escaped. This is how the READMEs write them.
shell_quote() {
  case "$1" in
    *"'"*) printf '"%s"' "$(printf '%s' "$1" | sed -e 's/[\\"$`]/\\&/g')" ;;
    *) printf "'%s'" "$1" ;;
  esac
}
# Random pod names, as a placeholder.
normalize() { sed -E 's/hello-[a-z0-9]{6,10}-[a-z0-9]{5}/hello-<pod>/g'; }

ask() { # host, prompt
  echo "\$ curl -s -H 'Host: $1' $URL/ -d $(shell_quote "$2")"
  curl -s -m 30 -H "Host: $1" "$URL/" -d "$2" | normalize
  echo
}

# Wait until a WorkloadDeployment runs the version just applied: its current
# replica set is the new one when the spec changed (a new generation), that
# replica set is the only one left, every one of its workloads is Ready, and
# no workload from an older replica set remains to answer a request.
rollout() { # namespace, name, generation before the apply, replica set before
  local ns=$1 name=$2 gen0=$3 rs0=$4 gen rs want ready others
  gen=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.metadata.generation}')
  for _ in $(seq 1 240); do
    rs=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.status.currentReplicaSet.name}' 2>/dev/null || true)
    if [ -n "$rs" ] && { [ "$gen" = "$gen0" ] || [ "$rs" != "$rs0" ]; }; then
      want=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.spec.replicas}')
      ready=$(kubectl -n "$ns" get workloads -l "runtime.wasmcloud.dev/workload-replicaset=$rs" \
        -o jsonpath='{range .items[*]}{.status.conditions[?(@.type=="Ready")].status}{"\n"}{end}' | grep -c '^True$' || true)
      others=$(kubectl -n "$ns" get workloads -o name | grep "/$name-" | grep -vc "/$rs-" || true)
      if [ "$ready" = "${want:-1}" ] && [ "$others" = 0 ]; then return 0; fi
    fi
    sleep 0.5
  done
  echo "timed out waiting for $ns/$name to roll out (generation $gen, replica set ${rs:-none})" >&2
  return 1
}

apply() { # manifest with one WorkloadDeployment: apply it and wait for the new version
  local ns name gen0 rs0
  echo "\$ kubectl apply -f $1 && $WAIT"
  ns=$(sed -n 's/^  namespace: //p' "$1" | head -1)
  ns=${ns:-default}
  name=$(sed -n 's/^  name: //p' "$1" | head -1)
  gen0=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.metadata.generation}' 2>/dev/null || echo 0)
  rs0=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.status.currentReplicaSet.name}' 2>/dev/null || true)
  kubectl apply -f "$1" >/dev/null
  rollout "$ns" "$name" "$gen0" "$rs0"
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
  echo "# Step 1: run the agent; three tools are ready and the kubectl slot waits"
  apply steps/01-run-it/workload.yaml
  echo "# sqlite reads the talks database:"
  ask agent.localhost 'list the talks in sqlite'
  echo "# git reads the repository:"
  ask agent.localhost 'show the recent commits'
  echo "# curl reaches the one host it is granted:"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
  echo "# The agent harness's imports from this workshop: the model and four typed, labelled tool slots"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'"
  wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
  echo "# The harness imports no HTTP client, no filesystem and no sockets, so this prints nothing:"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'"
  wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)' || true
  echo "# The tools are small. Look at the sizes:"
  echo "\$ ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm artifacts/tool_kubectl.wasm"
  ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm artifacts/tool_kubectl.wasm | awk '{print $5, $NF}'
} >"$(OUT 01-run-it)" 2>&1

{
  echo "# Step 2: the curl tool imports only an HTTP client; its reach is the grant"
  echo "# Path B, the prebuilt curl. Path A (your build) runs the same commands on"
  echo "# target/wasm32-wasip2/release/tool_curl.wasm and workload-mine.yaml."
  echo "\$ wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'"
  wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'
  apply steps/02-build-curl/workload.yaml
  echo "# It reaches the one host its manifest grants:"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
  echo "# It saves into its one folder:"
  ask agent.localhost 'save http://api.workshop-sites.svc/data to page.html'
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
  echo "# Step 4: sqlite over the databases in its read-only folder"
  apply steps/04-sqlite/workload.yaml
  echo "# A valid database on the host pod's disk that no workload is granted:"
  echo "\$ kubectl -n wasmcloud exec deploy/hostgroup-default -- ls /workshop/private-data"
  kubectl -n wasmcloud exec deploy/hostgroup-default -- ls /workshop/private-data
  echo "# Ordinary SQL works:"
  ask agent.localhost "sqlite select title, speaker from talks where track = 'Security' order by title"
  echo "# A dot-command, turned into SQL by the tool's shell:"
  ask agent.localhost 'sqlite .tables'
  echo "# ATTACH a database inside the grant: works"
  ask agent.localhost "sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms"
  echo "# ATTACH a valid database on the host pod, outside the grant: refused"
  ask agent.localhost "sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries"
  echo "# .shell: answered by the tool's own shell code"
  ask agent.localhost 'sqlite .shell id'
  echo "# load_extension: not compiled in"
  ask agent.localhost "sqlite select load_extension('/tmp/evil')"
  echo "# The imports: nothing that starts a process, no HTTP client, no sockets"
  echo "\$ wasm-tools component wit artifacts/tool_sqlite.wasm | grep -E '^  import'"
  wasm-tools component wit artifacts/tool_sqlite.wasm | grep -E '^  import'
} >"$(OUT 04-sqlite)" 2>&1

{
  echo "# Step 5: git reads a hostile repository, and none of its programs run"
  echo "# The repository's config sets core.fsmonitor, core.pager, aliases, filters,"
  echo "# a textconv, diff.external and a credential helper to marker commands."
  echo "# The tool has no import that could start a process:"
  echo "\$ wasm-tools component wit artifacts/tool_git.wasm | grep -E '^  import'"
  wasm-tools component wit artifacts/tool_git.wasm | grep -E '^  import'
  apply steps/05-git/workload.yaml
  echo "# log reads commits:"
  ask agent.localhost 'git log --oneline -5'
  echo "# status reports the branch and the tip:"
  ask agent.localhost 'git status'
  echo "# blame reads line history:"
  ask agent.localhost 'git blame README.md'
  echo "# push: refused by the tool's own dispatch"
  ask agent.localhost 'git push origin main'
} >"$(OUT 05-git)" 2>&1

{
  echo "# Step 5, native positive control: the same repository under this machine's git."
  echo "# Output depends on the local git version ($(git --version))."
  rm -f /tmp/gtfobins-git-*
  echo "\$ git -C .run/repo status"
  git -C .run/repo status 2>&1 || true
  echo "\$ ls /tmp/gtfobins-git-*"
  ls /tmp/gtfobins-git-* 2>&1 || true
  echo "\$ git -C .run/repo diff HEAD~2 HEAD"
  git -C .run/repo diff HEAD~2 HEAD 2>&1 || true
  echo "\$ ls /tmp/gtfobins-git-*"
  ls /tmp/gtfobins-git-* 2>&1 || true
  echo "\$ rm -f /tmp/gtfobins-git-*"
  rm -f /tmp/gtfobins-git-*
} >"$(CHECKS 05-git)" 2>&1

{
  echo "# Step 6: kubectl reads the demo namespace through the kube-api capability"
  apply steps/06-kubectl/workload.yaml
  echo "# Allowed (grant and RBAC): pods in demo"
  ask agent.localhost 'kubectl get pods'
  echo "# Allowed (RBAC): deployments"
  ask agent.localhost 'kubectl get deployments'
  echo "# Allowed (RBAC grants pods/log): a pod's logs"
  ask agent.localhost 'kubectl logs deployment/hello'
  echo "# Refused by the grant: curl may not reach the API server"
  ask agent.localhost 'curl -s https://kubernetes.default.svc/api/v1/namespaces/demo/pods'
  echo "# Refused by RBAC: secrets"
  ask agent.localhost 'kubectl get secrets'
  echo "# Refused by RBAC: every namespace"
  ask agent.localhost 'kubectl get pods -A'
  echo "# Refused by RBAC: kube-system"
  ask agent.localhost 'kubectl get pods -n kube-system'
  echo "# Refused by RBAC: the plugin forwards any method, the API server decides"
  ask agent.localhost 'kubectl raw DELETE /api/v1/namespaces/demo/pods/hello'
  echo "# The tool imports the kube-api capability and no HTTP client, sockets or secrets:"
  echo "\$ wasm-tools component wit artifacts/tool_kubectl.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasmcloud)'"
  wasm-tools component wit artifacts/tool_kubectl.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasmcloud)'
} >"$(OUT 06-kubectl)" 2>&1

{
  echo "# Step 6 control: the Role widened to read Secrets, nothing else changed"
  echo "\$ kubectl apply -f steps/06-kubectl/control-secrets-role.yaml"
  kubectl apply -f steps/06-kubectl/control-secrets-role.yaml
  ask agent.localhost 'kubectl get secrets'
  echo "\$ kubectl apply -f k8s/kube.yaml"
  kubectl apply -f k8s/kube.yaml >/dev/null
  echo "# Reverted: refused again"
  ask agent.localhost 'kubectl get secrets'
} >"$(CHECKS 06-kubectl)" 2>&1

{
  echo "# Extra: a real model behind the same slot (a fake OpenAI compatible endpoint)"
  apply extras/real-model/workload.yaml
  echo "# The model asks for a tool, the agent runs it, the model answers:"
  ask agent.localhost 'what talks are there?'
} >extras/real-model/expected-output.txt 2>&1

echo "proofs written:"
ls steps/*/expected-output.txt steps/*/control-checks.txt
